#![feature(box_syntax)]
use byteorder::{BigEndian, LittleEndian, ReadBytesExt, WriteBytesExt};
use std::{fs::File, io::Write, path::Path, str::FromStr};
use std::{
    io::{self, BufRead, Read},
    ops::DerefMut,
};

use io::Cursor;
use nix::{
    sys::uio::{process_vm_readv, process_vm_writev, IoVec, RemoteIoVec},
    unistd::Pid,
};

use error::{BetrayalError, BetrayalResult};
use procmaps;
use trickster::{Process, RegionPermissions};
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
        Err(error) => {
            return Err(BetrayalError::PartialRead);
        }
    };

    if bytes_read != bytes_requested {
        return Err(BetrayalError::PartialRead);
    }
    Ok(buffer)
}

pub type QueryResult = (usize, i32);

pub struct ProcessQuery {
    pub pid: i32,
    pub results: Vec<i32>,
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
            c.read_i32::<LittleEndian>()
                .map_err(|_e| BetrayalError::PartialRead)?,
        ))
    }

    fn query<'process, 'result>(
        &'process self,
        value: u32,
    ) -> BetrayalResult<Box<impl Iterator<Item = QueryResult> + 'result>>
    where
        'process: 'result,
    {
        let pid = self.pid;
        // let mappings = procmaps::Mappings::from_pid(pid).map_err(|_e| BetrayalError::BadPid)?.into_iter().collect::<Vec<_>>();
        let mappings = std::mem::take(
            procmaps::Mappings::from_pid(pid)
                .map_err(|_e| BetrayalError::BadPid)?
                .deref_mut(),
        );

        Ok(Box::new(
            mappings
                .into_iter()
                .flat_map(|map| (map.base..map.ceiling).step_by(4))
                .filter_map(move |address| Self::read_at(pid, address).ok()),
        ))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pid = take_input::<i32>("PID")?;

    // for mapping in maps.iter() {
    //     println!("region: {:x} - {:x}", mapping.base, mapping.ceiling);
    //     for val in (mapping.base..mapping.ceiling)
    //         .step_by(4)
    //         .take(10)
    //         .map(|start| {
    //             read_memory(pid, start, 4).and_then(|v| {
    //                 let mut c = Cursor::new(v);
    //                 Ok(c.read_u32::<LittleEndian>().ok())
    //             })
    //         })
    //         .filter_map(|v| v.ok())
    //         .filter_map(|v| v)
    //     {
    //         println!(" {}", val);
    //     }
    // }
    Ok(())
}
