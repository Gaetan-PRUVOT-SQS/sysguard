fn main() {
    let args: Vec<String> = std::env::args().collect();
    std::process::exit(sysguard::cli::sysguard_main(&args));
}
