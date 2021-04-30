#![feature(box_syntax)]

mod commands;
use byteorder::{NativeEndian, ReadBytesExt, WriteBytesExt};
use commands::Command;
use std::{
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
use procmaps;
mod error;
mod process;

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
                return Ok(());
            }
        }
        Err(e) => return Err(BetrayalError::BadWrite(format!("write error: {}", e))),
    }
}

pub type QueryResult = (usize, i32);

#[derive(Debug)]
pub struct ProcessQuery {
    pub pid: i32,
    pub results: Vec<QueryResult>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Filter {
    IsEqual(i32),
}

pub type Writer = (u32, i32);

impl Filter {
    pub fn matches(self, result: QueryResult) -> bool {
        let (_address, val) = result;
        match self {
            Self::IsEqual(v) => v == val,
        }
    }
}

impl ProcessQuery {
    pub fn new(pid: i32) -> Self {
        Self {
            pid,
            results: vec![],
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
        for (index, result) in self.results.iter_mut().enumerate() {
            let (address, _value) = result;
            match Self::read_at(self.pid, *address) {
                Ok(val) => *result = val,
                Err(_e) => invalid_regions.push(index),
            }
        }
        for index in invalid_regions.into_iter().rev() {
            self.results.remove(index);
        }
        Ok(())
    }

    pub fn perform_write(&mut self, writer: Writer) -> BetrayalResult<()> {
        let (index, value) = writer;
        let (address, _current_value) = self
            .results
            .get(index as usize)
            .ok_or(BetrayalError::BadWrite(format!("no such address")))?;
        Self::write_at(self.pid, *address, value)?;
        self.update_results()?;
        Ok(())
    }
    pub fn perform_query(&mut self, filter: Filter) -> BetrayalResult<()> {
        if self.results.len() == 0 {
            let results = self
                .query(filter)?
                .into_par_iter()
                .filter(|v| filter.matches(*v))
                .collect::<Vec<_>>();
            self.results = results;
            return Ok(());
        }
        self.update_results()?;
        self.results.retain(|v| filter.matches(*v));

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
        let mut mappings = std::mem::take(
            procmaps::Mappings::from_pid(pid)
                .map_err(|_e| BetrayalError::BadPid)?
                .deref_mut(),
        );
        mappings.retain(|m| m.perms.writable && m.perms.readable);
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
                std::thread::spawn(move || {
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
                            .filter(|result| filter.matches(*result))
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
        return Ok(results);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pid = take_input::<i32>("PID")?;

    let mut process = ProcessQuery::new(pid);
    loop {
        let input = take_input::<Command>("");
        match input {
            Ok(command) => match command {
                Command::Quit => break,
                Command::PerformFilter(filter) => process.perform_query(filter)?,
                Command::Write(writer) => process.perform_write(writer)?,
            },
            Err(e) => {
                eprintln!("{}", e);
                continue;
            }
        }

        for (index, (address, value)) in process.results.iter().enumerate() {
            println!("{}. 0x{:x} -- {}", index, address, value);
        }
    }
    println!("{:#?}", process);

    Ok(())
}
