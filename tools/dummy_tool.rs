fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1].contains("evil") {
        println!("Evil tool result");
    } else {
        println!("Fast tool result");
    }
}
