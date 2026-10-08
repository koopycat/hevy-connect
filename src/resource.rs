//! The Hevy resources the CLI exposes, and everything that differs between
//! them, as data. Commands are generic over this table.

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use crate::compact::Kind;
use crate::error::{Error, Result};
use crate::time::validate_date;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    List,
    Count,
    Events,
    View,
    History,
    Info,
    Create,
    Update,
}

impl Action {
    pub fn name(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Count => "count",
            Self::Events => "events",
            Self::View => "view",
            Self::History => "history",
            Self::Info => "info",
            Self::Create => "create",
            Self::Update => "update",
        }
    }

    /// Whether the action takes the record's identifier as its one positional.
    pub fn takes_id(self) -> bool {
        matches!(self, Self::View | Self::History | Self::Update)
    }
}

pub struct Spec {
    pub name: &'static str,
    /// The collection endpoint; a record lives at `{path}/{id}`.
    pub path: &'static str,
    /// `None` for the account, whose single record is not compacted.
    pub kind: Option<Kind>,
    /// What an identifier is called in errors.
    pub id_label: &'static str,
    /// The key a single-record response nests its record under.
    pub view_key: Option<&'static str>,
    /// The key a list response nests its records under, and a legacy alias.
    pub array_key: &'static str,
    pub legacy_array_key: Option<&'static str>,
    pub max_page_size: u64,
    pub default_page_size: Option<u64>,
    pub actions: &'static [Action],
}

use Action::{Count, Create, Events, History, Info, List, Update, View};

pub const RESOURCES: [Spec; 6] = [
    Spec {
        name: "user",
        path: "/v1/user/info",
        kind: None,
        id_label: "User ID",
        view_key: Some("data"),
        array_key: "",
        legacy_array_key: None,
        max_page_size: 0,
        default_page_size: None,
        actions: &[Info],
    },
    Spec {
        name: "workout",
        path: "/v1/workouts",
        kind: Some(Kind::Workout),
        id_label: "Workout ID",
        view_key: None,
        array_key: "workouts",
        legacy_array_key: None,
        max_page_size: 10,
        default_page_size: None,
        actions: &[List, Count, Events, View, Create, Update],
    },
    Spec {
        name: "routine",
        path: "/v1/routines",
        kind: Some(Kind::Routine),
        id_label: "Routine ID",
        view_key: Some("routine"),
        array_key: "routines",
        legacy_array_key: None,
        max_page_size: 10,
        default_page_size: None,
        actions: &[List, View, Create, Update],
    },
    Spec {
        name: "exercise",
        path: "/v1/exercise_templates",
        kind: Some(Kind::Exercise),
        id_label: "Exercise template ID",
        view_key: None,
        array_key: "exercise_templates",
        legacy_array_key: None,
        max_page_size: 100,
        default_page_size: Some(10),
        actions: &[List, View, History, Create],
    },
    Spec {
        name: "folder",
        path: "/v1/routine_folders",
        kind: Some(Kind::Folder),
        id_label: "Folder ID",
        view_key: None,
        array_key: "routine_folders",
        legacy_array_key: Some("routines"),
        max_page_size: 10,
        default_page_size: None,
        actions: &[List, View, Create],
    },
    Spec {
        name: "measurement",
        path: "/v1/body_measurements",
        kind: Some(Kind::Measurement),
        id_label: "Date",
        view_key: None,
        array_key: "body_measurements",
        legacy_array_key: None,
        max_page_size: 10,
        default_page_size: None,
        actions: &[List, View, Create, Update],
    },
];

pub fn find(name: &str) -> Option<&'static Spec> {
    RESOURCES.iter().find(|spec| spec.name == name)
}

impl Spec {
    pub fn action(&self, name: &str) -> Option<Action> {
        self.actions
            .iter()
            .copied()
            .find(|action| action.name() == name)
    }

    pub fn is_measurement(&self) -> bool {
        self.kind == Some(Kind::Measurement)
    }

    /// An identifier typed by the user, as one safe path segment.
    pub fn id_segment(&self, id: &str) -> Result<String> {
        if self.is_measurement() {
            return Ok(validate_date(id)?.to_owned());
        }
        encode_identifier(id, self.id_label)
    }

    /// The endpoint of one record.
    pub fn record_path(&self, id: &str) -> Result<String> {
        Ok(format!("{}/{}", self.path, self.id_segment(id)?))
    }
}

/// Everything `encodeURIComponent` leaves alone.
const IDENTIFIER_SAFE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

/// Percent-encode an identifier as one path segment. `.` and `..` are refused
/// because URL resolution would turn them into a different endpoint.
fn encode_identifier(value: &str, label: &str) -> Result<String> {
    if value.trim().is_empty()
        || value == "."
        || value == ".."
        || value.contains(['\0', '\r', '\n'])
    {
        return Err(Error::validation(format!("{label} is invalid.")));
    }
    Ok(utf8_percent_encode(value, IDENTIFIER_SAFE).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_one_safe_path_segment() {
        let workout = find("workout").unwrap();
        assert_eq!(
            workout.record_path("abc-123").unwrap(),
            "/v1/workouts/abc-123"
        );
        assert_eq!(
            workout.record_path("a/b c?d#e").unwrap(),
            "/v1/workouts/a%2Fb%20c%3Fd%23e"
        );
        assert_eq!(workout.record_path("ü").unwrap(), "/v1/workouts/%C3%BC");
        for bad in ["", "  ", ".", "..", "a\nb", "a\0b"] {
            assert_eq!(
                workout.record_path(bad).unwrap_err().message,
                "Workout ID is invalid.",
                "{bad:?}"
            );
        }
    }

    #[test]
    fn measurements_are_addressed_by_calendar_date() {
        let measurement = find("measurement").unwrap();
        assert_eq!(
            measurement.record_path("2024-08-14").unwrap(),
            "/v1/body_measurements/2024-08-14"
        );
        assert!(measurement.record_path("2024-02-30").is_err());
        assert!(measurement.record_path("today").is_err());
    }

    #[test]
    fn every_resource_has_actions_and_a_unique_name() {
        let mut names: Vec<_> = RESOURCES.iter().map(|spec| spec.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), RESOURCES.len());
        assert!(RESOURCES.iter().all(|spec| !spec.actions.is_empty()));
    }
}
