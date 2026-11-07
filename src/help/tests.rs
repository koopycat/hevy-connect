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
