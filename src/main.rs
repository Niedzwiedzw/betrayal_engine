#![feature(box_syntax)]

pub mod commands;
pub mod helpers;
pub mod memory;
pub mod neighbour_values;
pub mod reclass;
use crate::memory::ReadFromBytes;

use clap::{crate_version, App, Arg, Subcommand};
use commands::{Command, HELP_TEXT};
use itertools::Itertools;
use neighbour_values::NeighbourValuesQuery;
use parking_lot::Mutex;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::thread::JoinHandle;
use std::{collections::BTreeMap, fs::File, io::Write, path::Path, str::FromStr, sync::Arc};
use std::{
    io::{self, BufRead},
    ops::DerefMut,
};

use nix::{
    sys::uio::{process_vm_readv, process_vm_writev, IoVec, RemoteIoVec},
    unistd::Pid,
};

use error::{BetrayalError, BetrayalResult};
use procmaps::{self, Map};

use crate::neighbour_values::NeighbourValues;
mod error;
mod process;

pub fn take_input<T: FromStr>(prompt: &str) -> Result<T, <T as FromStr>::Err> {
    let mut input_string = String::new();
    print!("\n{} >> ", prompt);
    std::io::stdout().flush();
    std::io::stdin()
        .read_line(&mut input_string)
        .expect("Failed to read line");
    T::from_str(input_string.trim())
}

fn read_lines<P>(filename: P) -> io::Result<io::Lines<io::BufReader<File>>>
where
    P: AsRef<Path>,
{
    let file = File::open(filename)?;
    Ok(io::BufReader::new(file).lines())
}

pub fn read_memory(pid: i32, address: usize, bytes_requested: usize) -> BetrayalResult<Vec<u8>> {
    let mut buffer = vec![0u8; bytes_requested];
    let remote = RemoteIoVec {
        base: address,
        len: bytes_requested,
    };
    let bytes_read = match process_vm_readv(
        Pid::from_raw(pid),
        &[IoVec::from_mut_slice(&mut buffer)],
        &[remote],
    ) {
        Ok(bytes_read) => bytes_read,
        Err(_error) => {
            return Err(BetrayalError::PartialRead);
        }
    };

    if bytes_read != bytes_requested {
        return Err(BetrayalError::PartialRead);
    }
    Ok(buffer)
}

pub fn write_memory(pid: i32, address: usize, buffer: Vec<u8>) -> BetrayalResult<()> {
    let bytes_requested = buffer.len();
    let remote = RemoteIoVec {
        base: address,
        len: bytes_requested,
    };
    match process_vm_writev(Pid::from_raw(pid), &[IoVec::from_slice(&buffer)], &[remote]) {
        Ok(bytes_written) => {
            if bytes_written != bytes_requested {
                return Err(BetrayalError::BadWrite(format!(
                    "bad write length: {} != {}",
                    bytes_written, bytes_requested
                )));
            } else {
                Ok(())
            }
        }
        Err(e) => return Err(BetrayalError::BadWrite(format!("write error: {}", e))),
    }
}

pub type AddressValue<T: ReadFromBytes> = (AddressInfo, usize, T);

// #[derive(Debug)]
// pub enum AddressValueAs {
//     I32(AddressValue<i32>),
// }

pub type CurrentQueryResults<T: ReadFromBytes> = BTreeMap<usize, AddressValue<T>>;

#[derive(Debug)]
pub struct ProcessQuery<T: ReadFromBytes> {
    pub pid: i32,
    pub results: CurrentQueryResults<T>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Filter<T: ReadFromBytes> {
    IsEqual(T),
    InRange((T, T)),
    Any,
    ChangedBy(T),
    InAddressRanges(Vec<(usize, usize)>),
}

pub type Writer<T: ReadFromBytes> = (usize, T);

impl<T: ReadFromBytes> Filter<T> {
    pub fn matches(
        self,
        result: AddressValue<T>,
        current_results: &CurrentQueryResults<T>,
    ) -> bool {
        let (info, address, current_value) = result;
        match self {
            Self::IsEqual(v) => v == current_value,
            Self::InRange((base, ceiling)) => base <= current_value && current_value <= ceiling,
            Self::Any => true,
            Self::ChangedBy(diff) => current_results
                .get(&address)
                // .find(|(candidate_address, _value)| address == *candidate_address)
                .map(|(_info, _a, value)| current_value + diff == *value)
                .unwrap_or(false),
            Self::InAddressRanges(ranges) => ranges
                .iter()
                .any(|(base, ceiling)| base <= &address && &address <= ceiling),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddressInfo {
    pub writable: bool,
}

pub struct StaticLocation {
    pub map_path: String,
    pub offset: usize,
    pub base: usize,
}

impl AddressInfo {
    pub fn from_address(pid: i32, address: usize) -> BetrayalResult<Self> {
        let (info, _map) = ProcessQuery::<u8>::mappings_all(pid)?
            .into_iter()
            .find(|(_info, map)| map.base <= address && address < map.ceiling)
            .ok_or(BetrayalError::PartialRead)?;
        Ok(info)
    }

    pub fn is_static(&self) -> bool {
        !self.writable
    }

    pub fn static_location(&self, pid: i32, address: usize) -> Option<StaticLocation> {
        if !self.is_static() {
            return None;
        }
        let (_info, map) = ProcessQuery::<u8>::mappings_all(pid).ok()?.into_iter()
            .find(|(_info, map)| map.base <= address && address < map.ceiling)?;

        let path = map.pathname;
        match path {
            procmaps::Path::MappedFile(path) => Some(StaticLocation {
                map_path: path.clone(),
                base: map.base,
                offset: address - map.base,
            }),
            _ => None,
        }
    }
}

impl From<&Map> for AddressInfo {
    fn from(m: &Map) -> Self {
        Self {
            writable: m.perms.writable,
        }
    }
}

impl<T: ReadFromBytes> ProcessQuery<T> {
    pub fn new(pid: i32) -> Self {
        Self {
            pid,
            results: Default::default(),
        }
    }

    pub fn read_at(pid: i32, address: usize) -> BetrayalResult<AddressValue<T>> {
        let (info, _map) = Self::mappings_all(pid)?
            .into_iter()
            .find(|(info, m)| m.base <= address && address < m.ceiling)
            .ok_or(BetrayalError::PartialRead)?;
        let val = read_memory(pid, address, std::mem::size_of::<T>())?;
        Ok((
            info,
            address,
            T::read_value(val).map_err(|_e| BetrayalError::PartialRead)?,
        ))
    }

    pub fn write_at(pid: i32, address: usize, value: T) -> BetrayalResult<()> {
        let mut buffer = vec![];
        value
            .write_bytes(&mut buffer)
            .map_err(|e| BetrayalError::BadWrite(format!("bad write: {}", e)))?;
        write_memory(pid, address, buffer)?;
        Ok(())
    }

    pub fn update_results(&mut self) -> BetrayalResult<()> {
        let mut invalid_regions = vec![];
        {
            for (address, result) in self.results.iter_mut() {
                match Self::read_at(self.pid, *address) {
                    Ok(val) => *result = val,
                    Err(_e) => invalid_regions.push(*address),
                }
            }
        }
        for index in invalid_regions.into_iter().rev() {
            self.results.remove(&index);
        }
        Ok(())
    }

    pub fn perform_write(&mut self, writer: Writer<T>) -> BetrayalResult<()> {
        let (selected_address, value) = writer;
        let (_info, address, _current_value) = self
            .results
            .get(&selected_address)
            .ok_or(BetrayalError::BadWrite("no such address".to_string()))?;
        Self::write_at(self.pid, *address, value)?;
        self.update_results()?;
        Ok(())
    }

    pub fn perform_new_query(&mut self, filter: Filter<T>) -> BetrayalResult<()> {
        let results = self
            .query(filter.clone())?
            .into_par_iter()
            .filter(|v| filter.clone().matches(*v, &self.results))
            .map(|(info, address, value)| (address, (info, address, value)))
            .collect();
        self.results = results;
        Ok(())
    }
    pub fn perform_query(&mut self, filter: Filter<T>) -> BetrayalResult<()> {
        if self.results.is_empty() {
            self.perform_new_query(filter.clone())?;
        }
        let current_results = self.results.clone();
        self.update_results()?;
        self.results
            .retain(|_k, v| filter.clone().matches(*v, &current_results));

        Ok(())
    }

    pub fn mappings_all(pid: i32) -> BetrayalResult<Vec<(AddressInfo, Map)>> {
        let mut mappings = std::mem::take(
            procmaps::Mappings::from_pid(pid)
                .map_err(|_e| BetrayalError::BadPid)?
                .deref_mut(),
        );

        mappings.retain(|m| m.perms.readable);
        Ok(mappings
            .into_iter()
            .map(|m| {
                (
                    AddressInfo {
                        writable: m.perms.writable,
                    },
                    m,
                )
            })
            .collect())
    }

    fn mappings(&self) -> BetrayalResult<Vec<(AddressInfo, Map)>> {
        Self::mappings_all(self.pid)
    }
    fn all_possible_addresses(&self) -> BetrayalResult<Box<impl Iterator<Item = i32>>> {
        Ok(box self.mappings()?.into_iter().flat_map(|(_info, map)| {
            (map.base as i32)..((map.ceiling as i32) - std::mem::size_of::<i32>() as i32)
        }))
    }
    pub fn in_address_space(&self, value: i32) -> BetrayalResult<bool> {
        Ok(self
            .mappings()?
            .into_iter()
            .any(|(_info, map)| map.base as i32 <= value && value <= map.ceiling as i32))
    }

    fn query<'process, 'result>(
        &'process self,
        filter: Filter<T>,
        // ) -> BetrayalResult<Box<impl Iterator<Item = QueryResult> + 'result>>
    ) -> BetrayalResult<Vec<AddressValue<T>>>
    where
        'process: 'result,
    {
        let pid = self.pid;
        let mappings = self.mappings()?;
        let mappings: Vec<_> = mappings
            .into_iter()
            .unique_by(|(_info, m)| m.base)
            .unique_by(|(_info, m)| m.ceiling)
            .collect();

        let results: Arc<Mutex<Vec<AddressValue<T>>>> = Default::default();
        mappings.into_par_iter().for_each(|(info, map)| {
            let results = Arc::clone(&results);
            let filter = filter.clone();
            let dummy_results = Default::default(); // this should work for now cause this is only ran on the initial scan... I hope
            let mut results_chunk = match read_memory(pid, map.base, map.ceiling - map.base) {
                Ok(m) => T::possible_values(&m[..], map.base)
                    .map(|(address, value)| (info, address, value))
                    .filter(|result| filter.clone().matches(*result, &dummy_results))
                    .collect(),
                Err(_e) => {
                    vec![]
                }
            };
            results.lock().append(&mut results_chunk);
        });

        println!(" :: scanning done ::");
        let results = results.lock().clone();
        Ok(results)
    }
}

async fn run<T: 'static + ReadFromBytes>(
    pid: i32,
    tasks: &mut Vec<JoinHandle<()>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let process = ProcessQuery::<T>::new(pid);
    let process = Arc::new(Mutex::new(process));
    println!("{}", HELP_TEXT);
    println!(" :: running in [{}] mode", std::any::type_name::<T>());
    loop {
        let process = Arc::clone(&process);
        let input = take_input::<Command<T>>("");

        match input {
            Ok(command) => match command {
                Command::Quit => break,
                Command::Help => {
                    println!("{}", HELP_TEXT);
                    continue;
                }

                Command::Refresh => process.lock().update_results()?,
                Command::PerformFilter(filter) => process.lock().perform_query(filter)?,
                Command::Write(writer) => process.lock().perform_write(writer)?,
                Command::KeepWriting(writer) => {
                    let process = Arc::clone(&process);
                    tasks.push(std::thread::spawn(move || loop {
                        match process.lock().perform_write(writer) {
                            Ok(_) => {}
                            Err(e) => {
                                eprintln!(
                                    " :: [ERR] :: Writer thread crashed with {}. Aborting.",
                                    e
                                );
                                break;
                            }
                        };

                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }));
                }
                Command::AddAddress(address) => {
                    let mut process = process.lock();
                    let info = match AddressInfo::from_address(process.pid, address) {
                        Ok(v) => v,
                        Err(e) => {
                            eprintln!("error while adding address :: {}", e);
                            continue;
                        }
                    };
                    process
                        .results
                        .insert(address, (info, address, Default::default()));
                    process.update_results()?;
                }
                Command::AddAddressRange(start, end) => {
                    println!(" :: adding {} - {}", start, end);
                    let mut process = process.lock();
                    let info = match AddressInfo::from_address(process.pid, start) {
                        Ok(v) => v,
                        Err(e) => {
                            eprintln!("error while adding address :: {}", e);
                            continue;
                        }
                    };
                    for address in start..end {
                        process
                            .results
                            .insert(address, (info, address, Default::default()));
                    }
                    process.update_results()?;
                }
            },
            Err(e) => {
                eprintln!("{}", e);
                continue;
            }
        };
        if process.lock().results.len() > 50 {
            println!(":: found {} matches", process.lock().results.len());
        } else {
            let process = process.lock();
            for (index, (_, (info, address, value))) in process.results.iter().enumerate() {
                println!(
                    "{}. {} (0x{:x}) -- {} {}",
                    index,
                    address,
                    address,
                    value,
                    match info.static_location(process.pid, *address) {
                        Some(location) => format!("@STATIC[static_address(\"{}\")+{}] (raw: {} + {})", location.map_path, location.offset, location.base, location.offset),
                        None => String::new()
                    }
                    // if info.is_static() {
                    //     match 
                    //     let location = info.static_location(process.pid, *address);
                    //     format!("@STATIC()")

                    // } else { String::new() }
                );
            }
        }
    }

    println!("{:#?}", process);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let matches = App::new("Betrayal Engine")
        .version(crate_version!())
        .author("Niedźwiedź <wojciech.brozek@niedzwiedz.it>")
        .about("A fast, lightweight memory searcher and editor")
        .subcommand(
            App::new("reclass")
                .about("reclass-like interface for finding structs")
        )
        .arg(
            Arg::new("pid")
                .short('p')
                .long("pid")
                .value_name("INT")
                .required(true)
                .about("PID of the process you're interested in analyzing"),
        )
        .arg(
            Arg::new("variable_type")
                .short('t')
                .long("variable_type")
                .value_name("u8 | u16 | u16 | i32 | u32 | i64 | u64 | f32 | f64")
                .default_value("i32")
                .about("currently you need to specify the format up front and only use that until the end of the program. but hey, you can always run multiple instances of this thing. oh yeah and i32 is 32 bits signed, equivalent of 4 bytes in other software"),
        )
        .get_matches();
    let pid = matches.value_of_t_or_exit("pid");
    println!("PID: {}", pid);
    if let Some(ref _matches) = matches.subcommand_matches("reclass") {
        reclass::run::run(pid)?;
        std::process::exit(0);
    }
    let mut tasks = vec![];
    match matches.value_of("variable_type") {
        Some(t) => match t.trim() {
            "u8" => run::<u8>(pid, &mut tasks).await?,
            "i16" => run::<i16>(pid, &mut tasks).await?,
            "u16" => run::<u16>(pid, &mut tasks).await?,
            "i32" => run::<i32>(pid, &mut tasks).await?,
            "u32" => run::<u32>(pid, &mut tasks).await?,
            "i64" => run::<i64>(pid, &mut tasks).await?,
            "u64" => run::<u64>(pid, &mut tasks).await?,
            "f32" => run::<f32>(pid, &mut tasks).await?,
            "f64" => run::<f64>(pid, &mut tasks).await?,
            _ => panic!("unsupported variable type"),
        },
        None => {
            panic!("variable_type is required");
        }
    }
    run::<i32>(pid, &mut tasks).await?;
    Ok(())
}
