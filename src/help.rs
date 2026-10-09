//! Help text. The pages live in `src/help/*.txt`; blocks shared between pages
//! are written once and substituted here.

const SHARED: [(&str, &str); 3] = [
    ("{{COMMON}}", include_str!("help/common.txt")),
    ("{{PAGINATION}}", include_str!("help/pagination.txt")),
    ("{{MUTATION}}", include_str!("help/mutation.txt")),
];

fn page(command: &str) -> Option<&'static str> {
    Some(match command {
        "user" => include_str!("help/user.txt"),
        "workout" => include_str!("help/workout.txt"),
        "routine" => include_str!("help/routine.txt"),
        "exercise" => include_str!("help/exercise.txt"),
        "folder" => include_str!("help/folder.txt"),
        "measurement" => include_str!("help/measurement.txt"),
        "setup" => include_str!("help/setup.txt"),
        _ => return None,
    })
}

fn expand(template: &str) -> String {
    let text = SHARED
        .iter()
        .fold(template.to_owned(), |text, (marker, block)| {
            text.replace(marker, block.trim_end())
        });
    text.trim_end().to_owned()
}

pub fn top_level() -> String {
    expand(include_str!("help/top.txt"))
}

pub fn command(name: &str) -> Option<String> {
    page(name).map(expand)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::RESOURCES;

    #[test]
    fn every_command_has_a_page_with_no_unresolved_blocks() {
        let commands = RESOURCES.iter().map(|spec| spec.name).chain(["setup"]);
        for name in commands {
            let help = command(name).unwrap_or_else(|| panic!("no help for {name}"));
            assert!(help.starts_with("Usage: hevy-axi "), "{name}");
            assert!(!help.contains("{{"), "{name} has an unresolved block");
            assert!(
                top_level().contains(name),
                "{name} is missing from the top-level help"
            );
        }
        assert!(command("nope").is_none());
    }

    #[test]
    fn every_action_is_documented_on_its_resource_page() {
        for spec in &RESOURCES {
            let help = command(spec.name).unwrap();
            for action in spec.actions {
                assert!(
                    help.contains(&format!("  {}", action.name())),
                    "{} {}",
                    spec.name,
                    action.name()
                );
            }
        }
    }
}
