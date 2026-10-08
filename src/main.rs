fn main() -> std::process::ExitCode {
    hevy_axi::run(std::env::args_os().skip(1))
}
