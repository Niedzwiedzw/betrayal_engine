use crate::{BetrayalError, Writer};
use crate::{error::BetrayalResult, Filter};
use nom::{bytes::complete::take_while, error::ParseError, IResult};
use std::str::FromStr;
// take_input<T: FromStr>(prompt: &str) -> Result<T, <T as FromStr>::Err>

// const KEYWORDS: [&str; 2] = ["f", "e"];

#[derive(PartialEq, Eq, Debug)]
pub enum Command {
    PerformFilter(Filter),
    Write(Writer),
    Quit,
}

// fn is_command_prefix(i: &str) -> bool {
//     KEYWORDS.iter().any(|keyword| &i == keyword)
// }

// fn space<'a, E: ParseError<&'a str>>(i: &'a str) -> IResult<&'a str, &'a str, E> {
//     take_while(move |c| c == ' ')(i)
// }

fn command_parser(i: &str) -> BetrayalResult<Command> {
    let command = i.split_whitespace().collect::<Vec<_>>();
    match &command[..] {
        ["q"] => Ok(Command::Quit),
        ["w", index, value] => Ok(Command::Write((
            index
                .parse()
                .map_err(|e| BetrayalError::BadCommand(format!("invalid value: {}", e)))?,
            value
                .parse()
                .map_err(|e| BetrayalError::BadCommand(format!("invalid value: {}", e)))?,
        ))),
        ["f", compare, value] => Ok(Command::PerformFilter(match compare {
            &"e" => Filter::IsEqual(
                value
                    .parse()
                    .map_err(|e| BetrayalError::BadCommand(format!("invalid value: {}", e)))?,
            ),
            _ => return Err(BetrayalError::BadCommand(format!("command not found"))),
        })),
        _ => Err(BetrayalError::BadCommand(format!("command not found"))),
    }
}

impl FromStr for Command {
    type Err = BetrayalError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Ok(command_parser(s).map_err(|e| BetrayalError::BadCommand(e.to_string()))?.1)
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
