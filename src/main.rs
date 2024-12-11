pub mod commands;
pub mod helpers;
pub mod memory;
pub mod neighbour_values;
pub mod reclass;
use {
    crate::memory::ReadFromBytes,
    clap::{crate_version, App, Arg},
    commands::{Command, HELP_TEXT},
    error::{BetrayalError, BetrayalResult},
    itertools::Itertools,
    nix::{
        sys::uio::{process_vm_readv, process_vm_writev, IoVec, RemoteIoVec},
        unistd::Pid,
    },
    ordered_float::OrderedFloat,
    parking_lot::Mutex,
    petgraph::{data::Build, graph::NodeIndex},
    procmaps::{self, Map},
    rayon::prelude::*,
    serde::{Deserialize, Serialize},
    std::{
        collections::{BTreeMap, BTreeSet},
        convert::{TryFrom, TryInto},
        fs::File,
        io::{self, BufRead, Write},
        ops::DerefMut,
        path::Path,
        str::FromStr,
        sync::Arc,
        thread::JoinHandle,
    },
};

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
    let bytes_read = match process_vm_readv(Pid::from_raw(pid), &[IoVec::from_mut_slice(&mut buffer)], &[remote]) {
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
                return Err(BetrayalError::BadWrite(format!("bad write length: {} != {}", bytes_written, bytes_requested)));
            } else {
                Ok(())
            }
        }
        Err(e) => return Err(BetrayalError::BadWrite(format!("write error: {}", e))),
    }
}

pub type AddressValue<T> = (AddressInfo, usize, T);

// #[derive(Debug)]
// pub enum AddressValueAs {
//     I32(AddressValue<i32>),
// }

pub type CurrentQueryResults<T> = BTreeMap<usize, AddressValue<T>>;

#[derive(Debug)]
pub struct ProcessQuery<T: ReadFromBytes> {
    pub pid: i32,
    pub results: CurrentQueryResults<T>,
    pub mappings: Vec<(AddressInfo, Map)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Filter<T: ReadFromBytes> {
    IsEqual(T),
    InRange((T, T)),
    Any,
    ChangedBy(T),
    InAddressRanges(Vec<(usize, usize)>),
    IsInValueBox(usize, usize, Arc<BTreeSet<T>>),
}

pub type Writer<T> = (usize, T);

impl<T: ReadFromBytes> Filter<T> {
    pub fn matches(self, result: AddressValue<T>, current_results: &CurrentQueryResults<T>) -> bool {
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

            Self::IsInValueBox(base, ceiling, values) => (base <= address && address <= ceiling) && values.contains(&current_value),
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
    pub fn from_address<T: memory::ReadFromBytes>(process: &ProcessQuery<T>, pid: i32, address: usize) -> BetrayalResult<Self> {
        let (info, _map) = process
            .mappings()?
            .into_iter()
            .find(|(_info, map)| map.base <= address && address < map.ceiling)
            .ok_or(BetrayalError::PartialRead)?;
        Ok(info.clone())
    }

    pub fn is_static(&self) -> bool {
        !self.writable
    }

    /// file with permission RW, either with a name, or directly following a named map (without a gap!!)
    pub fn static_location(&self, pid: i32, address: usize) -> Option<StaticLocation> {
        use procmaps::Path;
        if !self.writable {
            return None;
        }
        let maps = ProcessQuery::<u8>::mappings_all_with_unreadable(pid).ok()?;
        let slice_index = match maps
            .iter()
            .find_position(|(_info, map)| map.base <= address && address < map.ceiling)
        {
            Some((map_index, (_, map))) => match &map.pathname {
                Path::MappedFile(name) if name == "" && map.base == maps.get(map_index - 1)?.1.ceiling => {
                    // this is a .bss, we start one address up
                    Some(map_index - 1)
                }
                Path::MappedFile(_) => Some(map_index), // this is a normal mapped file, still we need to offset it to allow for lookups
                _ => None,
            },
            None => None,
        }?;

        let maps = (&maps[..(slice_index + 1)]) // omit later entries
            .iter()
            .sorted_by_key(|(_, m)| m.base)
            .rev() // go backwards
            .collect::<Vec<_>>();
        let static_base = maps
            .iter()
            .zip(maps.iter().skip(1)) // compare neighbours
            .inspect(|((_, curr), (_, next))| {
                // println!(
                //     "({:x} - {:x}) :: {:?}\n({:x} - {:x}) :: {:?}",
                //     curr.base, curr.ceiling, curr.pathname, next.base, next.ceiling, next.pathname
                // )
            })
            .take_while(|((_, curr), (_, next))| curr.base == next.ceiling) // there can be no memory gap
            .collect::<Vec<_>>()
            .into_iter()
            .map(|((_, curr), (_, _))| curr)
            .group_by(|m| &m.pathname) // chunks of maps with the same path
            .into_iter()
            .next()
            .map(|(_, v)| v)?
            .last()?;

        let path = &static_base.pathname;
        match path {
            procmaps::Path::MappedFile(path) => Some(StaticLocation {
                map_path: path.clone(),
                base: static_base.base,
                offset: address - static_base.base,
            }),
            _ => None,
        }
    }
}

impl From<&Map> for AddressInfo {
    fn from(m: &Map) -> Self {
        Self { writable: m.perms.writable }
    }
}

#[extension_traits::extension(pub trait MapExt)]
impl Map {
    fn contains(&self, addr: usize) -> bool {
        self.base <= addr && addr <= self.ceiling
    }
}

pub fn find_equal_to<T: ReadFromBytes>(pid: i32, value: T) -> BetrayalResult<Vec<AddressValue<T>>> {
    let mut process = ProcessQuery::<T>::new(pid);
    process.perform_new_query(Filter::IsEqual(value))?;
    Ok(process.results.into_iter().map(|(_k, v)| v).collect())
}

pub fn find_in_range<T: ReadFromBytes>(pid: i32, min: T, max: T) -> BetrayalResult<Vec<AddressValue<T>>> {
    let mut process = ProcessQuery::<T>::new(pid);
    process.perform_new_query(Filter::InRange((min, max)))?;
    Ok(process.results.into_iter().map(|(_k, v)| v).collect())
}

use petgraph::graph::DiGraph;

fn log_graph<T: ReadFromBytes + Serialize + TryFrom<usize>>(graph: &DiGraph<T, ()>, pid: i32) {
    for edge in graph.node_indices() {
        print!("[*]");
        let mut dfs = petgraph::visit::Dfs::new(&graph, edge);

        while let Some(visited) = dfs.next(&graph) {
            let address = graph[visited];
            print!(" -> {:?}", address);
        }
        println!();
    }

    println!();
}

pub fn build_pointer_tree<T: 'static + ReadFromBytes + Serialize + TryFrom<usize>>(
    pid: i32,
    tree: Arc<Mutex<DiGraph<T, ()>>>,
    current: Option<NodeIndex>,
    addresses: Vec<T>,
    depth: T,
) -> BetrayalResult<()> {
    let mut tasks = vec![];
    for address in addresses {
        let tree = Arc::clone(&tree);
        let a = {
            let mut tree = tree.lock();
            let a = tree.add_node(address);
            if let Some(current) = current {
                tree.add_edge(a, current, ());
            }
            a
        };

        let addresses = find_in_range(pid, address - depth, address)?
            .into_iter()
            .filter_map(|(_, a, _)| a.try_into().ok())
            .collect();
        tasks.push(std::thread::spawn(move || build_pointer_tree(pid, tree, Some(a), addresses, depth)));
    }
    for task in tasks {
        task.join()
            .map_err(|e| BetrayalError::BadWrite(format!("thread crashed during pointer map building :: {:#?}", e)))??;
    }
    Ok(())
}

pub fn pointer_map<T: 'static + ReadFromBytes + Serialize + TryFrom<usize>>(pid: i32, address: T, depth: T) -> BetrayalResult<DiGraph<T, ()>> {
    let graph = Default::default();
    build_pointer_tree::<T>(pid, Arc::clone(&graph), None, vec![address], depth)?;
    let graph = graph.lock().clone();
    Ok(graph)
}

impl<T: ReadFromBytes> ProcessQuery<T> {
    pub fn new(pid: i32) -> Self {
        Self {
            pid,
            results: Default::default(),
            mappings: Default::default(),
        }
    }

    pub fn read_at(&mut self, pid: i32, address: usize) -> BetrayalResult<AddressValue<T>> {
        if self.mappings.is_empty() {
            self.update_mappings()?; // oof
        }
        let (info, _map) = self
            .mappings()?
            .into_iter()
            .find(|(info, m)| m.base <= address && address < m.ceiling)
            .ok_or(BetrayalError::PartialRead)?;
        let val = read_memory(pid, address, std::mem::size_of::<T>())?;
        Ok((info.clone(), address, T::read_value(val).map_err(|_e| BetrayalError::PartialRead)?))
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
        let mut results = self.results.clone();
        {
            for (address, result) in results.iter_mut() {
                match self.read_at(self.pid, *address) {
                    Ok(val) => *result = val,
                    Err(_e) => invalid_regions.push(*address),
                }
            }
        }
        for index in invalid_regions.into_iter().rev() {
            results.remove(&index);
        }
        self.results = results;

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

    pub fn mappings_all_with_unreadable(pid: i32) -> BetrayalResult<Vec<(AddressInfo, Map)>> {
        let mappings = std::mem::take(
            procmaps::Mappings::from_pid(pid)
                .map_err(|_e| BetrayalError::BadPid)?
                .deref_mut(),
        );
        Ok(mappings
            .into_iter()
            .map(|m| (AddressInfo { writable: m.perms.writable }, m))
            .collect())
    }

    pub fn mappings_all(pid: i32) -> BetrayalResult<Vec<(AddressInfo, Map)>> {
        Ok(Self::mappings_all_with_unreadable(pid)?
            .into_iter()
            .collect())
    }

    fn mappings(&self) -> BetrayalResult<Box<impl Iterator<Item = &(AddressInfo, Map)>>> {
        Ok(Box::new(self.mappings.iter()))
    }

    pub fn in_address_space(&self, value: i32) -> BetrayalResult<bool> {
        Ok(self
            .mappings()?
            .into_iter()
            .any(|(_info, map)| map.base as i32 <= value && value <= map.ceiling as i32))
    }

    pub fn update_mappings(&mut self) -> BetrayalResult<()> {
        self.mappings = Self::mappings_all(self.pid)?;
        Ok(())
    }
    fn query<'process, 'result>(
        &'process mut self,
        filter: Filter<T>,
        // ) -> BetrayalResult<Box<impl Iterator<Item = QueryResult> + 'result>>
    ) -> BetrayalResult<Vec<AddressValue<T>>>
    where
        'process: 'result,
    {
        self.update_mappings()?;

        let pid = self.pid;
        let mappings = self.mappings()?;
        let mut mappings: Vec<_> = mappings
            .into_iter()
            .unique_by(|(_info, m)| m.base)
            .unique_by(|(_info, m)| m.ceiling)
            .collect();

        match &filter {
            Filter::IsInValueBox(start, end, arc) => {
                mappings.retain(|(_, map)| map.contains(*start) || map.contains(*end));
            }
            //
            Filter::InAddressRanges(vec) => {}
            Filter::IsEqual(_) => {}
            Filter::InRange(_) => {}
            Filter::Any => {}
            Filter::ChangedBy(_) => {}
        }

        let results: Arc<Mutex<Vec<AddressValue<T>>>> = Default::default();
        mappings.into_par_iter().for_each(|(info, map)| {
            let results = Arc::clone(&results);
            let filter = filter.clone();
            let dummy_results = Default::default(); // this should work for now cause this is only ran on the initial scan... I hope
            let mut results_chunk = match read_memory(pid, map.base, map.ceiling - map.base) {
                Ok(m) => T::possible_values(&m[..], map.base)
                    .map(|(address, value)| (info.clone(), address, value))
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

async fn run<T: 'static + ReadFromBytes>(pid: i32, tasks: &mut Vec<JoinHandle<()>>) -> Result<(), Box<dyn std::error::Error>> {
    let mut process = ProcessQuery::<T>::new(pid);
    process.update_mappings()?;
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
                                eprintln!(" :: [ERR] :: Writer thread crashed with {}. Aborting.", e);
                                break;
                            }
                        };

                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }));
                }
                Command::AddAddress(address) => {
                    let mut process = process.lock();
                    let info = match AddressInfo::from_address(&process, process.pid, address) {
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
                    let info = match AddressInfo::from_address(&process, process.pid, start) {
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
                Command::PointerMapU32(address, depth) => {
                    println!(" :: building a pointer32 map for {}", address);
                    let pid = { process.lock().pid };
                    let mut map = match pointer_map::<u32>(pid, address, depth) {
                        Ok(map) => map,
                        Err(e) => {
                            println!(" :: ERR :: {}", e);
                            continue;
                        }
                    };
                    println!(" :: SUCCESS ::",);
                    log_graph(&mut map, pid)
                }
                Command::PointerMapU64(address, depth) => {
                    println!(" :: building a pointer64 map for {}", address);
                    let pid = { process.lock().pid };
                    let mut map = match pointer_map::<u64>(pid, address, depth) {
                        Ok(map) => map,
                        Err(e) => {
                            println!(" :: ERR :: {}", e);
                            continue;
                        }
                    };
                    println!(" :: SUCCESS ::",);
                    log_graph(&mut map, pid)
                }
                Command::FindValuesInBox(start, end, values) => process
                    .lock()
                    .perform_query(Filter::IsInValueBox(start, end, Arc::new(values.into_iter().collect())))?,
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
                        Some(location) => format!(
                            "@STATIC[static_address(PID,\"{}\")+{}] (raw: {} + {})",
                            location.map_path, location.offset, location.base, location.offset
                        ),
                        None => String::new(),
                    } // if info.is_static() {
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
        .subcommand(App::new("reclass").about("reclass-like interface for finding structs"))
        .arg(
            Arg::new("pid")
                .short('p')
                .long("pid")
                .value_name("INT")
                .required(true)
                .help("PID of the process you're interested in analyzing"),
        )
        .arg(
            Arg::new("variable_type")
                .short('t')
                .long("variable_type")
                .value_name("u8 | u16 | u16 | i32 | u32 | i64 | u64")
                .default_value("i32")
                .help(
                    "currently you need to specify the format up front and only use that until the end of the program. but hey, you can always run multiple instances of this \
                     thing. oh yeah and i32 is 32 bits signed, equivalent of 4 bytes in other software",
                ),
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
            "f32" => run::<OrderedFloat<f32>>(pid, &mut tasks).await?,
            "f64" => run::<OrderedFloat<f64>>(pid, &mut tasks).await?,
            _ => panic!("unsupported variable type"),
        },
        None => {
            panic!("variable_type is required");
        }
    }
    run::<i32>(pid, &mut tasks).await?;
    Ok(())
}
