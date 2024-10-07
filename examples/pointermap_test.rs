// #![feature(lazy_cell)]

// use std::sync::{Arc, Mutex};
// use std::{io::Write, str::FromStr};

// use std::cell::LazyCell;

// static GAME: Arc<LazyCell<Mutex<Game>>> = Arc::new(LazyCell::new(|| {
//     Mutex::new(Game {
//         something: 12,
//         world: Box::new(World {
//             time: 11,
//             weather: 23,
//             player: Box::new(Player { hp: 100, mana: 100 }),
//         }),
//     })
// }));

// pub fn take_input<T: FromStr>(prompt: &str) -> Result<T, <T as FromStr>::Err> {
//     let mut input_string = String::new();
//     print!("\n{} >> ", prompt);
//     std::io::stdout().flush();
//     std::io::stdin()
//         .read_line(&mut input_string)
//         .ok()
//         .expect("Failed to read line");
//     T::from_str(input_string.trim())
// }

// #[repr(C)]
// #[derive(Debug)]
// struct Player {
//     pub mana: i32,
//     pub hp: i32,
// }
// #[repr(C)]
// #[derive(Debug)]
// struct World {
//     pub time: i64,
//     pub weather: i64,
//     pub player: Box<Player>,
// }
// #[repr(C)]
// #[derive(Debug)]
// struct Game {
//     pub something: i32,
//     pub world: Box<World>,
// }

// fn main() -> Result<(), Box<dyn std::error::Error>> {
//     loop {
//         println!("{:#?}", GAME.lock()?);
//         take_input::<String>("").unwrap();
//         GAME.lock()?.world.player.hp -= 3;
//     }
// }
fn main() {}
