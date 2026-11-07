use super::*;

#[test]
fn paths_under_home_are_collapsed() {
    let home = Path::new("/Users/vk");
    assert_eq!(
        collapse_home(Path::new("/Users/vk/.local/bin/hevy-axi"), home),
        "~/.local/bin/hevy-axi"
    );
    assert_eq!(
        collapse_home(Path::new("/Users/vk2/bin/hevy-axi"), home),
        "/Users/vk2/bin/hevy-axi"
    );
    assert_eq!(
        collapse_home(Path::new("/usr/bin/hevy-axi"), home),
        "/usr/bin/hevy-axi"
    );
}

#[test]
fn usage_names_the_action_and_its_shape() {
    assert_eq!(
        usage("workout", "view", true, false),
        "hevy-axi workout view <id> [flags]"
    );
    assert_eq!(
        usage("folder", "create", false, true),
        "hevy-axi folder create --file <path|-> (--confirm|--dry-run)"
    );
}
