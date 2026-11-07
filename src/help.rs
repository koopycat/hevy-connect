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
mod tests;
