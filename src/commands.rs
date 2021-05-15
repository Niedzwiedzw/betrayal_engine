use crate::{error::BetrayalResult, Filter};
use crate::{BetrayalError, Writer};
use nom::{bytes::complete::take_while, error::ParseError, IResult};
use std::str::FromStr;

#[derive(PartialEq, Eq, Debug)]
pub enum Command {
    PerformFilter(Filter),
    KeepWriting(Writer),
    Write(Writer),
    Quit,
    FindStructsReferencing(i32, usize),
    Refresh,
    Help,
}


macro_rules! parse_or_bad_command {
	  ($value:expr) => {
		    $value.parse().map_err(|e| BetrayalError::BadCommand(format!("invalid value: {}", e)))?
	  };
}

pub const HELP_TEXT: &str = r#"
[ :: Betrayal Engine :: ]
author: wojciech.brozek@niedzwiedz.it
github: https://github.com/Niedzwiedzw/betratal_engine

COMMANDS:
""                       -> refreshes current results
"q"                      -> quits the program
"h" or "?" or "help"     -> prints this help message
"w <index> <value>"      -> writes a specified value to address at results

"k <index> <value>"      -> same as "w" but does that in a loop so that value stays the same (god mode etc)
"s s <address> <depth>"  -> finds structs referencing that address and adds their fields to the results (BETA)
"f u"                    -> a NO-OP filter, for new scans it will match all the values (very memory intensive), equivalent to refresh for subsequent scans
"f e 2137"               -> finds values equal to 2137
"f c 15"                 -> finds values that changed by 15 compared to previous scan (does nothing for initial scan)
"#;

fn command_parser(i: &str) -> BetrayalResult<Command> {
    let command = i.split_whitespace().collect::<Vec<_>>();
    match &command[..] {
        [] => Ok(Command::Refresh),
        ["h" | "?" | "help"] => Ok(Command::Help),
        ["q"] => Ok(Command::Quit),
        ["w", index, value] => Ok(Command::Write((
            parse_or_bad_command!(index),
            parse_or_bad_command!(value),
        ))),

        ["k", index, value] => Ok(Command::KeepWriting((
            parse_or_bad_command!(index),
            parse_or_bad_command!(value),
        ))),
        ["s", "s", address, depth] => {
            Ok(Command::FindStructsReferencing(parse_or_bad_command!(address), parse_or_bad_command!(depth)))
        },
        ["f", "u"] => Ok(Command::PerformFilter(Filter::Any)),
        ["f", compare, value] => Ok(Command::PerformFilter(match *compare {
            "e" => Filter::IsEqual(
                parse_or_bad_command!(value)
            ),
            "c" => Filter::ChangedBy(
                parse_or_bad_command!(value)
            ),
            _ => return Err(BetrayalError::BadCommand("command not found".to_string())),
        })),
        _ => Err(BetrayalError::BadCommand("command not found".to_string())),
    }
}

impl FromStr for Command {
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
            "f e 44".parse::<Command>().unwrap(),
            Command::PerformFilter(Filter::IsEqual(44))
        )
    }

    #[test]
    fn test_quit() {
        assert_eq!("q".parse::<Command>().unwrap(), Command::Quit,)
    }

    #[test]
    fn test_write() {
        assert_eq!(
            "w 3 2137".parse::<Command>().unwrap(),
            Command::Write((3, 2137)),
        )
    }
}
