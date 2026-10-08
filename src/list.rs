//! Paginated listing: one page, a limited number of items, or every page.

use serde_json::{Map, Value, json};

use crate::args::Parsed;
use crate::client::Client;
use crate::compact::Kind;
use crate::config::Environment;
use crate::error::{Error, Result};
use crate::output::{OutputOptions, project_fields, shell_argument};
use crate::resource::Spec;
use crate::wire::{array, object, safe_integer};

const MAX_AUTO_PAGES: u64 = 500;
pub const MAX_AUTO_ITEMS: usize = 5000;

/// What differs between lists of different resources.
pub struct ListSpec {
    pub resource: &'static str,
    /// The action word in continuation commands: `list` or `events`.
    pub action: &'static str,
    pub path: &'static str,
    pub array_key: &'static str,
    pub legacy_array_key: Option<&'static str>,
    pub kind: Kind,
    pub max_page_size: u64,
    pub default_page_size: Option<u64>,
    pub extra_query: Vec<(&'static str, String)>,
    /// Flags that reproduce caller-supplied filters in continuation commands.
    pub continuation: Vec<String>,
    /// Ask Hevy for an exact total when the pages read do not give one.
    pub counts_total: bool,
    /// Whether `view` is a sensible next step from this list.
    pub has_view: bool,
}

impl ListSpec {
    pub fn of(spec: &Spec) -> Self {
        Self {
            resource: spec.name,
            action: "list",
            path: spec.path,
            array_key: spec.array_key,
            legacy_array_key: spec.legacy_array_key,
            kind: spec.kind.expect("a listed resource has a kind"),
            max_page_size: spec.max_page_size,
            default_page_size: spec.default_page_size,
            extra_query: Vec::new(),
            continuation: Vec::new(),
            counts_total: spec.name == "workout",
            has_view: true,
        }
    }
}

/// The paging flags, validated before any request is made.
struct Paging {
    page: u64,
    page_size: u64,
    all: bool,
    limit: Option<usize>,
}

impl Paging {
    fn from_args(parsed: &Parsed, spec: &ListSpec) -> Result<Self> {
        let all = parsed.switch("all");
        let page = parsed.positive_integer("page")?;
        // A smaller default page keeps a single page compact; --all wants the
        // fewest requests.
        let page_size = parsed
            .positive_integer("page-size")?
            .or(if all { None } else { spec.default_page_size })
            .unwrap_or(spec.max_page_size);
        let limit = parsed.positive_integer("limit")?;
        if page_size > spec.max_page_size {
            return Err(Error::validation(format!(
                "--page-size must not exceed {}.",
                spec.max_page_size
            )));
        }
        if all && page.is_some() {
            return Err(Error::validation("--all cannot be combined with --page."));
        }
        if limit.is_some_and(|l| l > MAX_AUTO_ITEMS as u64) {
            return Err(Error::validation(format!(
                "--limit must not exceed {MAX_AUTO_ITEMS}."
            )));
        }
        Ok(Self {
            page: page.unwrap_or(1),
            page_size,
            all,
            limit: limit.map(|l| l as usize),
        })
    }
}

fn pagination_error() -> Error {
    Error::protocol("Hevy returned invalid pagination metadata.")
}

/// The items of one list response, tolerating the envelope variations seen live.
fn page_items(wire: &Map<String, Value>, spec: &ListSpec) -> Result<Vec<Value>> {
    let description = format!("{} array", spec.resource);
    if spec.kind == Kind::Event
        && !wire.get(spec.array_key).is_some_and(Value::is_array)
        && wire.get("workouts").is_some_and(Value::is_array)
    {
        // Without `since`, Hevy has answered with plain workouts under `workouts`.
        return wire["workouts"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|item| match item.get("type") {
                Some(Value::String(_)) => Ok(item.clone()),
                _ => object(item, "legacy workout event")
                    .map(|_| json!({ "type": "updated", "workout": item })),
            })
            .collect();
    }
    let documented = wire.get(spec.array_key);
    let items = if documented.is_none() {
        spec.legacy_array_key.and_then(|key| wire.get(key))
    } else {
        documented
    };
    Ok(array(items, &description)?.clone())
}

pub fn list(
    env: &Environment,
    parsed: &Parsed,
    spec: &ListSpec,
    options: &OutputOptions,
) -> Result<Value> {
    let paging = Paging::from_args(parsed, spec)?;
    let client = Client::configured(env)?;
    let start_page = if paging.all { 1 } else { paging.page };
    let mut current = start_page;
    let mut fetched_pages = 0;
    let mut seen = 0;
    let mut items: Vec<Value> = Vec::new();

    // The loop ends with the page count Hevy reported and how the last page
    // was split by the limit: items returned, and items left out.
    let (page_count, accepted_from_page, omitted_from_page) = loop {
        if fetched_pages >= MAX_AUTO_PAGES {
            return Err(Error::validation(format!(
                "Automatic pagination exceeds the safety cap of {MAX_AUTO_PAGES} pages."
            )));
        }
        let mut query = vec![
            ("page", current.to_string()),
            ("pageSize", paging.page_size.to_string()),
        ];
        query.extend(
            spec.extra_query
                .iter()
                .map(|(name, value)| (*name, value.clone())),
        );
        let response = client.get(spec.path, &query)?;
        let wire = object(&response, &format!("{} list response", spec.resource))?;

        let (wire_page, wire_count) = (
            safe_integer(wire.get("page")),
            safe_integer(wire.get("page_count")),
        );
        let empty_page = wire_page == Some(1) && wire_count == Some(0);
        let page_count = match (wire_page, wire_count) {
            (Some(page), Some(count)) if page == current && (empty_page || count >= page) => count,
            _ => return Err(pagination_error()),
        };
        let page_items = page_items(wire, spec)?;
        if empty_page && !page_items.is_empty() {
            return Err(pagination_error());
        }

        // Record what a limit leaves out of this page, so that `hasMore` and the
        // continuation command never silently skip items.
        let room = paging
            .limit
            .map_or(page_items.len(), |limit| limit.saturating_sub(items.len()));
        let accepted = page_items.len().min(room);
        if items.len() + accepted > MAX_AUTO_ITEMS {
            return Err(Error::validation(format!(
                "Automatic pagination exceeds the safety cap of {MAX_AUTO_ITEMS} items."
            )));
        }
        seen += page_items.len();
        let omitted = page_items.len() - accepted;
        items.extend(page_items.into_iter().take(accepted));
        fetched_pages += 1;

        if !paging.all
            || current >= page_count
            || paging.limit.is_some_and(|limit| items.len() >= limit)
        {
            break (page_count, accepted, omitted);
        }
        // Progress by the requested page, independently of potentially clamped or
        // stale response metadata. The page and item caps remain the final guard.
        current += 1;
    };

    let has_more = current < page_count || omitted_from_page > 0;
    // A limit that ends inside a page resumes at that same page, whose first
    // `skip` items were already returned.
    let resume =
        (omitted_from_page > 0).then(|| json!({ "page": current, "skip": accepted_from_page }));
    let next_page = if resume.is_some() {
        current
    } else {
        current + 1
    };

    let mut output = Map::new();
    output.insert("page".into(), start_page.into());
    output.insert("pageCount".into(), page_count.into());
    output.insert("resultCount".into(), items.len().into());
    output.insert("empty".into(), items.is_empty().into());
    output.insert("hasMore".into(), has_more.into());
    if let Some(resume) = resume {
        output.insert("resume".into(), resume);
    }
    if options.full {
        output.insert(spec.array_key.into(), Value::Array(items));
        return Ok(Value::Object(output));
    }

    let compact = items
        .iter()
        .map(|item| spec.kind.compact(item, false))
        .collect::<Result<Vec<_>>>()?;
    let default_fields: Vec<String> = spec
        .kind
        .default_fields()
        .iter()
        .map(|f| (*f).to_owned())
        .collect();
    let fields = options.fields.as_deref().unwrap_or(&default_fields);
    let results = project_fields(
        &Value::Array(compact),
        fields,
        Some(&spec.kind.available_fields()),
    )?;
    output.insert("results".into(), results);

    if paging.all && current >= page_count {
        // Every page was read, so the count is exact even when a limit applied.
        output.insert("totalCount".into(), seen.into());
    } else if spec.counts_total {
        output.insert(
            "totalCount".into(),
            crate::read::workout_count(&client)?.into(),
        );
    }

    let mut command = vec![
        "hevy-axi".to_owned(),
        spec.resource.to_owned(),
        spec.action.to_owned(),
    ];
    command.extend(
        spec.continuation
            .iter()
            .map(|argument| shell_argument(argument)),
    );
    let command = command.join(" ");
    let help = if has_more {
        vec![format!(
            "{command} --page {next_page} --page-size {}",
            paging.page_size
        )]
    } else {
        let mut help = vec![format!("{command} --all")];
        if spec.has_view {
            let id = if spec.kind == Kind::Measurement {
                "<date>"
            } else {
                "<id>"
            };
            help.push(format!("hevy-axi {} view {id}", spec.resource));
        }
        help
    };
    output.insert("help".into(), json!(help));
    Ok(Value::Object(output))
}
