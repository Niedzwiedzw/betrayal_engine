#![feature(box_syntax)]

mod commands;
mod helpers;
mod memory;
mod neighbour_values;

use byteorder::{NativeEndian, ReadBytesExt, WriteBytesExt};
use commands::{Command, HELP_TEXT};
use itertools::Itertools;
use neighbour_values::NeighbourValuesQuery;
use parking_lot::Mutex;
use rayon::prelude::*;
use std::{collections::BTreeMap, fs::File, io::Write, path::Path, str::FromStr, sync::Arc};
use std::{
    io::{self, BufRead, Read},
    ops::DerefMut,
};

use io::Cursor;
use nix::{
    sys::uio::{process_vm_readv, process_vm_writev, IoVec, RemoteIoVec},
    unistd::Pid,
};
use rayon::prelude::*;

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

pub type AddressValue<T> = (usize, T);

pub type CurrentQueryResults = BTreeMap<usize, AddressValue<i32>>;

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

pub type Writer = (usize, i32);

impl Filter {
    pub fn matches(self, result: AddressValue<i32>, current_results: &CurrentQueryResults) -> bool {
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

    fn read_at(pid: i32, address: usize) -> BetrayalResult<AddressValue<i32>> {
        let val = read_memory(pid, address, std::mem::size_of::<i32>())?;
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
        let (selected_address, value) = writer;
        let (address, _current_value) = self
            .results
            .get(&selected_address)
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
            .flat_map(|map| (map.base as i32)..((map.ceiling as i32) - std::mem::size_of::<i32>() as i32)))
    }
    pub fn in_address_space(&self, value: i32) -> BetrayalResult<bool> {
        Ok(self
            .mappings()?
            .into_iter()
            .any(|map| map.base as i32 <= value && value <= map.ceiling as i32))
    }

    fn query<'process, 'result>(
        &'process self,
        filter: Filter,
        // ) -> BetrayalResult<Box<impl Iterator<Item = QueryResult> + 'result>>
    ) -> BetrayalResult<Vec<AddressValue<i32>>>
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

        let results: Arc<Mutex<Vec<AddressValue<i32>>>> = Default::default();
        mappings.into_par_iter().for_each(|map| {
            let results = Arc::clone(&results);
            let filter = filter.clone();
            let dummy_results = Default::default(); // this should work for now cause this is only ran on the initial scan... I hope
            let mut results_chunk = match read_memory(pid, map.base, map.ceiling - map.base) {
                Ok(m) => memory::possible_values_i32(&m[..], map.base)
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

    pub fn find_neighbour_values(
        &self,
        NeighbourValuesQuery {
            values,
            window_size,
        }: NeighbourValuesQuery,
    ) -> BetrayalResult<Vec<NeighbourValues>> {
        let pid = self.pid;
        let mappings = self.mappings()?;
        let mappings: Vec<_> = mappings
            .into_iter()
            .unique_by(|m| m.base)
            .unique_by(|m| m.ceiling)
            .collect();
        let results: Arc<Mutex<Vec<NeighbourValues>>> = Default::default();
        mappings.into_par_iter().for_each(|map| {
            let results = Arc::clone(&results);
            let window_size = window_size.clone();
            let pid = pid.clone();
            let values = values.clone();
            // let dummy_results = Default::default(); // this should work for now cause this is only ran on the initial scan... I hope
            let mut results_chunk = match read_memory(pid, map.base, map.ceiling - map.base) {
                Ok(m) => memory::possible_values_i32(&m[..], map.base)
                    .enumerate()
                    .map(|(i, v)| (i % std::mem::size_of::<i32>(), v))
                    .sorted_by_key(|(phase, _v)| *phase)
                    .group_by(|(phase, _v)| *phase)
                    .into_iter()
                    .filter_map(|(_, phase)| {
                        let phase = phase.into_iter().map(|(_, v)| v).collect();
                        let next = helpers::windowed(&phase, window_size)
                            .filter(|window| {
                                values.iter().all(|v| {
                                    window.iter().any(|(_, memory_value)| memory_value == v)
                                })
                            })
                            .map(|v| v.iter().cloned().collect())
                            .next();
                        next
                    })
                    .map(|values| NeighbourValues {
                        window_size,
                        values,
                    })
                    .collect(),
                Err(_e) => {
                    vec![]
                }
            };
            results.lock().append(&mut results_chunk);
            // index
        });

        println!(" :: scanning done ::");
        let results = results.lock().clone();
        Ok(results)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pid = take_input::<i32>("PID")?;
    println!("PID: {}", pid);
    let process = ProcessQuery::new(pid);
    let process = Arc::new(Mutex::new(process));
    println!("{}", HELP_TEXT);
    let mut tasks = vec![];
    loop {
        let process = Arc::clone(&process);
        let input = take_input::<Command>("");
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
                Command::FindNeighbourValues(query) => {
                    let process = process.lock();
                    let results = process.find_neighbour_values(query)?;
                    println!(" :: NEIGHBOUR VALUES ::");
                    for result in results {
                        println!("{:#?}", result);
                    }
                }
                Command::AddAddress(address) => {
                    let mut process = process.lock();
                    process.results.insert(address, (address, 0));
                    process.update_results()?;
                }
                Command::AddAddressRange(start, end) => {
                    println!(" :: adding {} - {}", start, end);
                    let mut process = process.lock();
                    for address in start..end {
                        process.results.insert(address, (address, 0));
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
            for (index, (_, (address, value))) in process.lock().results.iter().enumerate() {
                println!("{}. {} (0x{:x}) -- {}", index, address, address, value);
            }
        }
    }
    println!("{:#?}", process);

    Ok(())
}
