
use std::{io::Write, str::FromStr};

pub fn take_input<T: FromStr>(prompt: &str) -> Result<T, <T as FromStr>::Err> {
    let mut input_string = String::new();
    print!("\n{} >> ", prompt);
    std::io::stdout().flush();
    std::io::stdin()
        .read_line(&mut input_string)
        .ok()
        .expect("Failed to read line");
    T::from_str(input_string.trim())
}
fn main() {
    let mut value: u32 = 15;
    for _ in 0..2137 {
        value += 1;
        println!("value: {}", value);
        take_input::<String>("").unwrap();
    }
}
