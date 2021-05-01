#![feature(box_syntax)]

mod commands;
use byteorder::{NativeEndian, ReadBytesExt, WriteBytesExt};
use commands::{Command, HELP_TEXT};
use std::{
    collections::BTreeMap,
    fs::File,
    io::Write,
    path::Path,
    str::FromStr,
    sync::{Arc, Mutex},
};
use std::{
    io::{self, BufRead, Read},
    ops::DerefMut,
};

use itertools::Itertools;

use io::Cursor;
use nix::{
    sys::uio::{process_vm_readv, process_vm_writev, IoVec, RemoteIoVec},
    unistd::Pid,
};
use rayon::prelude::*;

use error::{BetrayalError, BetrayalResult};
use procmaps::{self, Map};
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

pub type QueryResult = (usize, i32);
pub type CurrentQueryResults = BTreeMap<usize, QueryResult>;
#[derive(Debug)]
pub struct ProcessQuery {
    pub pid: i32,
    pub results: CurrentQueryResults,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Filter {
    IsEqual(i32),
    InRange((i32, i32)),
    Any,
    ChangedBy(i32),
    InAddressRanges(Vec<(usize, usize)>),
}

pub type Writer = (u32, i32);

impl Filter {
    pub fn matches(self, result: QueryResult, current_results: &CurrentQueryResults) -> bool {
        let (address, current_value) = result;
        match self {
            Self::IsEqual(v) => v == current_value,
            Self::InRange((base, ceiling)) => base <= current_value && current_value <= ceiling,
            Self::Any => true,
            Self::ChangedBy(diff) => current_results
                .get(&address)
                // .find(|(candidate_address, _value)| address == *candidate_address)
                .map(|(_a, value)| current_value + diff == *value)
                .unwrap_or(false),
            Self::InAddressRanges(ranges) => ranges
                .iter()
                .any(|(base, ceiling)| base <= &address && &address <= ceiling),
        }
    }
}

impl ProcessQuery {
    pub fn new(pid: i32) -> Self {
        Self {
            pid,
            results: Default::default(),
        }
    }

    fn read_at(pid: i32, address: usize) -> BetrayalResult<QueryResult> {
        let val = read_memory(pid, address, 4)?;
        let mut c = Cursor::new(val);
        Ok((
            address,
            c.read_i32::<NativeEndian>()
                .map_err(|_e| BetrayalError::PartialRead)?,
        ))
    }

    fn write_at(pid: i32, address: usize, value: i32) -> BetrayalResult<()> {
        let mut buffer = vec![];
        buffer
            .write_i32::<NativeEndian>(value)
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

    pub fn perform_write(&mut self, writer: Writer) -> BetrayalResult<()> {
        let (index, value) = writer;
        let (selected_address, _current_value) =
            self.results
                .iter()
                .nth((index) as usize)
                .ok_or(BetrayalError::BadWrite(
                    "Address is no longer valid".to_string(),
                ))?;
        let (address, _current_value) = self
            .results
            .get(selected_address)
            .ok_or(BetrayalError::BadWrite("no such address".to_string()))?;
        Self::write_at(self.pid, *address, value)?;
        self.update_results()?;
        Ok(())
    }

    pub fn perform_new_query(&mut self, filter: Filter) -> BetrayalResult<()> {
        let results = self
            .query(filter.clone())?
            .into_par_iter()
            .filter(|v| filter.clone().matches(*v, &self.results))
            .map(|(address, value)| (address, (address, value)))
            .collect();
        self.results = results;
        Ok(())
    }
    pub fn perform_query(&mut self, filter: Filter) -> BetrayalResult<()> {
        if self.results.is_empty() {
            self.perform_new_query(filter.clone())?;
        }
        let current_results = self.results.clone();
        self.update_results()?;
        self.results
            .retain(|_k, v| filter.clone().matches(*v, &current_results));

        Ok(())
    }
    fn mappings(&self) -> BetrayalResult<Vec<Map>> {
        let pid = self.pid;
        let mut mappings = std::mem::take(
            procmaps::Mappings::from_pid(pid)
                .map_err(|_e| BetrayalError::BadPid)?
                .deref_mut(),
        );

        mappings.retain(|m| m.perms.writable && m.perms.readable);
        Ok(mappings)
    }
    fn all_possible_addresses(&self) -> BetrayalResult<Box<impl Iterator<Item = i32>>> {
        Ok(box self
            .mappings()?
            .into_iter()
            .flat_map(|map| (map.base as i32)..((map.ceiling as i32) - 4)))
    }
    pub fn in_address_space(&self, value: i32) -> BetrayalResult<bool> {
        Ok(self
            .mappings()?
            .into_iter()
            .any(|map| map.base as i32 <= value && value <= map.ceiling as i32))
    }

    pub fn find_structs_referencing(&mut self, address: usize, depth: usize) -> BetrayalResult<()> {
        if !self.in_address_space(address as i32)? {
            println!(":: {} not in address space", address);
            return Ok(());
        }
        let ranges = self
            .query(Filter::InRange(((address - depth) as i32, address as i32)))?
            .into_iter()
            .map(|(address, _value)| (address - depth, address + depth / 2))
            .collect::<Vec<_>>();
        println!(":: found {} potential structs", ranges.len());
        self.perform_new_query(Filter::InAddressRanges(ranges))?;

        Ok(())
    }

    fn query<'process, 'result>(
        &'process self,
        filter: Filter,
        // ) -> BetrayalResult<Box<impl Iterator<Item = QueryResult> + 'result>>
    ) -> BetrayalResult<Vec<QueryResult>>
    where
        'process: 'result,
    {
        let pid = self.pid;
        let mappings = self.mappings()?;
        let mappings: Vec<_> = mappings
            .into_iter()
            .unique_by(|m| m.base)
            .unique_by(|m| m.ceiling)
            .collect();
        let scannable = mappings.len();
        let mut left_to_scan = scannable;

        let results: Arc<Mutex<Vec<QueryResult>>> = Default::default();
        let tasks: Vec<std::thread::JoinHandle<_>> = mappings
            .into_iter()
            .enumerate()
            .map(|(index, map)| {
                let results = Arc::clone(&results);
                let filter = filter.clone();
                std::thread::spawn(move || {
                    let dummy_results = Default::default(); // this should work for now cause this is only ran on the initial scan... I hope
                    let mut results_chunk = match read_memory(pid, map.base, map.ceiling - map.base)
                    {
                        Ok(memory) => (0..(map.ceiling - map.base - 3))
                            .filter_map(move |index| {
                                match Cursor::new(&memory[index..index + 4])
                                    .read_i32::<NativeEndian>()
                                {
                                    Ok(value) => Some((map.base + index, value)),
                                    Err(_e) => None,
                                }
                            })
                            .filter(|result| filter.clone().matches(*result, &dummy_results))
                            .collect::<Vec<_>>(),
                        Err(_e) => {
                            vec![]
                        }
                    };
                    results
                        .lock()
                        .expect("a previous thread crashed while accessing mutex...")
                        .append(&mut results_chunk);
                    index
                })
            })
            .collect();

        for task in tasks {
            let _index = task.join();
            left_to_scan -= 1;
            print!(
                "\r :: {} / {} regions scanned     ",
                scannable - left_to_scan,
                scannable
            );
        }
        let results = results.lock().expect("some thread crashed").clone();
        Ok(results)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pid = take_input::<i32>("PID")?;

    let mut process = ProcessQuery::new(pid);
    println!("{}", HELP_TEXT);
    loop {
        let input = take_input::<Command>("");
        match input {
            Ok(command) => match command {
                Command::Quit => break,
                Command::Help => {
                    println!("{}", HELP_TEXT);
                    continue;
                },
                Command::Refresh => process.update_results()?,
                Command::PerformFilter(filter) => process.perform_query(filter)?,
                Command::Write(writer) => process.perform_write(writer)?,
                Command::FindStructsReferencing(address, depth) => {
                    process.find_structs_referencing(address as usize, depth)?
                }
            },
            Err(e) => {
                eprintln!("{}", e);
                continue;
            }
        }
        if process.results.len() > 50 {
            println!(":: found {} matches", process.results.len());
        } else {
            for (index, (_, (address, value))) in process.results.iter().enumerate() {
                println!("{}. 0x{:x} ({}) -- {}", index, address, address, value);
            }
        }
    }
    println!("{:#?}", process);

    Ok(())
}
