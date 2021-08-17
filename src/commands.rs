use crate::memory::ReadFromBytes;
use crate::{error::BetrayalResult, Filter};
use crate::{BetrayalError, Writer};
use std::str::FromStr;

#[derive(PartialEq, Eq, Debug)]
pub enum Command<T: ReadFromBytes> {
    PerformFilter(Filter<T>),
    KeepWriting(Writer<T>),
    Write(Writer<T>),
    Quit,
    Refresh,
    Help,
    AddAddress(usize),
    AddAddressRange(usize, usize),
}


macro_rules! parse_or_bad_command {
    ($value:expr) => {
        $value
            .parse()
            .map_err(|_e| BetrayalError::BadCommand(format!("invalid value")))?
    };
}

pub const HELP_TEXT: &str = r#"
[ :: Betrayal Engine :: ]
author: wojciech.brozek@niedzwiedz.it
github: https://github.com/Niedzwiedzw/betratal_engine

COMMANDS:
""                       -> refreshes current results
"a <address> <address?>  -> adds address to the list (or range of addresses if second argument is present)
"q"                      -> quits the program
"h" or "?" or "help"     -> prints this help message
"w <index> <value>"      -> writes a specified value to address at results
"n <window_size> 1 2 14" -> lists all possible windows of <window_size> that contain 1 2 and 14 in no specific order, useful for finding structs
"k <index> <value>"      -> same as "w" but does that in a loop so that value stays the same (god mode etc)
"f u"                    -> a NO-OP filter, for new scans it will match all the values (very memory intensive), equivalent to refresh for subsequent scans
"f e 2137"               -> finds values equal to 2137
"f c 15"                 -> finds values that changed by 15 compared to previous scan (does nothing for initial scan)
"f r 15 300"             -> finds values between 15 and 300
"#;

fn command_parser<T: ReadFromBytes>(i: &str) -> BetrayalResult<Command<T>> {
    let command = i.split_whitespace().collect::<Vec<_>>();
    match &command[..] {
        [] => Ok(Command::Refresh),
        ["h" | "?" | "help"] => Ok(Command::Help),
        ["q"] => Ok(Command::Quit),
        ["w", index, value] => Ok(Command::Write((
            parse_or_bad_command!(index),
            parse_or_bad_command!(value),
        ))),
        ["a", address] => Ok(Command::AddAddress(parse_or_bad_command!(address))),
        ["a", address_start, address_end] => Ok(Command::AddAddressRange(
            parse_or_bad_command!(address_start),
            parse_or_bad_command!(address_end),
        )),
        ["k", index, value] => Ok(Command::KeepWriting((
            parse_or_bad_command!(index),
            parse_or_bad_command!(value),
        ))),
        ["f", "u"] => Ok(Command::PerformFilter(Filter::Any)),
        ["f", compare, value] => Ok(Command::PerformFilter(match *compare {
            "e" => Filter::IsEqual(parse_or_bad_command!(value)),
            "c" => Filter::ChangedBy(parse_or_bad_command!(value)),
            _ => return Err(BetrayalError::BadCommand("command not found".to_string())),
        })),
        ["f", "r", start, end] => Ok(Command::PerformFilter(Filter::InRange((
            parse_or_bad_command!(start),
            parse_or_bad_command!(end),
        )))),
        _ => Err(BetrayalError::BadCommand("command not found".to_string())),
    }
}

impl<T: ReadFromBytes> FromStr for Command<T> {
    type Err = BetrayalError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        command_parser(s)
    }
}

#[cfg(test)]
mod test_command_parsing {
    use super::*;

    #[test]
    fn test_filter_command() {
        assert_eq!(
            "f e 44".parse::<Command<i32>>().unwrap(),
            Command::PerformFilter(Filter::IsEqual(44))
        )
    }

    #[test]
    fn test_quit() {
        assert_eq!("q".parse::<Command<i32>>().unwrap(), Command::Quit,)
    }

    #[test]
    fn test_write() {
        assert_eq!(
            "w 3 2137".parse::<Command<i32>>().unwrap(),
            Command::Write((3, 2137)),
        )
    }
}
