extern crate crossbeam_channel as channel;
#[cfg(feature = "selfupdate")]
#[macro_use]
extern crate self_update;

use std::borrow::ToOwned;
use std::env;
use std::fmt;
use std::io;
use std::process;

use serde::Deserialize;

use docopt::Docopt;

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

macro_rules! wout {
    ($($arg:tt)*) => ({
        use std::io::Write;
        (writeln!(&mut ::std::io::stdout(), $($arg)*)).unwrap();
    });
}

macro_rules! werr {
    ($($arg:tt)*) => ({
        use std::io::Write;
        (writeln!(&mut ::std::io::stderr(), $($arg)*)).unwrap();
    });
}

macro_rules! fail {
    ($e:expr) => {
        Err(::std::convert::From::from($e))
    };
}

macro_rules! command_list {
    () => {
        "
    apply*      Apply series of transformations to a column
    behead      Drop header from CSV file
    cat         Concatenate by row or column
    count       Count records
    dedup       Remove redundant rows
    enum        Add a new column enumerating CSV lines
    exclude     Excludes the records in one CSV from another
    explode     Explode rows based on some column separator
    fill        Fill empty values
    fixlengths  Makes all records have same length
    flatten     Show one field per line
    fmt         Format CSV output (change field delimiter)
    foreach*    Loop over a CSV file to execute bash commands (*nix only)
    frequency   Show frequency tables
    generate*   Generate test data by profiling a CSV
    headers     Show header names
    help        Show this usage message.
    index       Create CSV index for faster access
    input       Read CSV data with special quoting rules
    join        Join CSV files
    jsonl       Convert newline-delimited JSON files to CSV
    lua*        Execute Lua script on CSV data
    partition   Partition CSV data based on a column value
    pseudo      Pseudonymise the values of a column
    sample      Randomly sample CSV data
    rename      Rename the columns of CSV data efficiently
    replace     Replace patterns in CSV data
    reverse     Reverse rows of CSV data
    search      Search CSV data with a regex
    searchset   Search CSV data with a regex set
    select      Select, re-order, duplicate or drop columns
    slice       Slice records from CSV
    sort        Sort CSV data in alphabetical, numerical, reverse or random order
    split       Split CSV data into many files
    stats       Infer data types and compute descriptive statistics
    table       Align CSV data into columns
    transpose   Transpose rows/columns of CSV data

    * optional feature
"
    };
}
mod cmd;
mod config;
mod index;
mod select;
mod util;

static USAGE: &str = concat!(
    "
Usage:
    qsv <command> [<args>...]
    qsv [options]

Options:
    --list        List all commands available.
    --envlist     List all environment variables with the QSV_ prefix.
    --update      Update qsv to the latest release (requires selfupdate feature).
    -h, --help    Display this message
    <command> -h  Display the command help message
    --version     Print version info, mem allocator, max_jobs, num_cpus then exit

Commands:",
    command_list!()
);

#[derive(Deserialize)]
struct Args {
    arg_command: Option<Command>,
    flag_list: bool,
    flag_envlist: bool,
    #[allow(dead_code)]
    flag_update: bool,
}

#[cfg(feature = "selfupdate")]
fn get_exec_name() -> Option<String> {
    std::env::current_exe()
        .ok()
        .and_then(|pb| pb.file_name().map(|s| s.to_os_string()))
        .and_then(|s| s.into_string().ok())
}

#[cfg(feature = "selfupdate")]
fn qsv_update() -> Result<(), Box<dyn ::std::error::Error>> {
    let exec_name = get_exec_name().unwrap();
    let status = self_update::backends::github::Update::configure()
        .repo_owner("jqnatividad")
        .repo_name("qsv")
        .bin_name(&exec_name)
        .show_download_progress(true)
        .show_output(true)
        .no_confirm(false)
        .current_version(cargo_crate_version!())
        .build()?
        .update()?;
    let exe_full_path = format!("{:?}", std::env::current_exe().unwrap());
    println!(
        "Update status for {}: `{}`!",
        exe_full_path,
        status.version()
    );
    Ok(())
}

fn main() {
    let args: Args = Docopt::new(USAGE)
        .and_then(|d| {
            d.options_first(true)
                .version(Some(util::version()))
                .deserialize()
        })
        .unwrap_or_else(|e| e.exit());
    if args.flag_list {
        wout!(concat!("Installed commands:", command_list!()));
        return;
    } else if args.flag_envlist {
        util::show_env_vars();
        return;
    }
    #[cfg(feature = "selfupdate")]
    if args.flag_update {
        if let Err(err) = qsv_update() {
            werr!("{}", err);
            ::std::process::exit(1);
        }
        return;
    }
    match args.arg_command {
        None => {
            werr!(concat!(
                "qsv is a suite of CSV command line utilities.

Please choose one of the following commands:",
                command_list!()
            ));
            process::exit(0);
        }
        Some(cmd) => match cmd.run() {
            Ok(()) => process::exit(0),
            Err(CliError::Flag(err)) => err.exit(),
            Err(CliError::Csv(err)) => {
                werr!("{}", err);
                process::exit(1);
            }
            Err(CliError::Io(ref err)) if err.kind() == io::ErrorKind::BrokenPipe => {
                process::exit(0);
            }
            Err(CliError::Io(err)) => {
                werr!("{}", err);
                process::exit(1);
            }
            Err(CliError::Other(msg)) => {
                werr!("{}", msg);
                process::exit(1);
            }
        },
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Command {
    #[cfg(feature = "apply")]
    Apply,
    Behead,
    Cat,
    Count,
    Dedup,
    Enum,
    Exclude,
    Explode,
    Fill,
    FixLengths,
    Flatten,
    Fmt,
    #[cfg(feature = "foreach")]
    ForEach,
    Frequency,
    #[cfg(feature = "generate")]
    Generate,
    Headers,
    Help,
    Index,
    Input,
    Join,
    Jsonl,
    #[cfg(feature = "lua")]
    Lua,
    Partition,
    Pseudo,
    Rename,
    Replace,
    Reverse,
    Sample,
    Search,
    SearchSet,
    Select,
    Slice,
    Sort,
    Split,
    Stats,
    Table,
    Transpose,
}

impl Command {
    fn run(self) -> CliResult<()> {
        let argv: Vec<_> = env::args().collect();
        let argv: Vec<_> = argv.iter().map(|s| &**s).collect();
        let argv = &*argv;

        if !argv[1].chars().all(char::is_lowercase) {
            return Err(CliError::Other(format!(
                "qsv expects commands in lowercase. Did you mean '{}'?",
                argv[1].to_lowercase()
            )));
        }
        match self {
            Command::Behead => cmd::behead::run(argv),
            #[cfg(feature = "apply")]
            Command::Apply => cmd::apply::run(argv),
            Command::Cat => cmd::cat::run(argv),
            Command::Count => cmd::count::run(argv),
            Command::Dedup => cmd::dedup::run(argv),
            Command::Enum => cmd::enumerate::run(argv),
            Command::Exclude => cmd::exclude::run(argv),
            Command::Explode => cmd::explode::run(argv),
            Command::Fill => cmd::fill::run(argv),
            Command::FixLengths => cmd::fixlengths::run(argv),
            Command::Flatten => cmd::flatten::run(argv),
            Command::Fmt => cmd::fmt::run(argv),
            Command::Frequency => cmd::frequency::run(argv),
            #[cfg(feature = "generate")]
            Command::Generate => cmd::generate::run(argv),
            Command::Headers => cmd::headers::run(argv),
            Command::Help => {
                wout!("{}", USAGE);
                Ok(())
            }
            Command::Index => cmd::index::run(argv),
            Command::Input => cmd::input::run(argv),
            Command::Join => cmd::join::run(argv),
            Command::Jsonl => cmd::jsonl::run(argv),
            #[cfg(feature = "lua")]
            Command::Lua => cmd::lua::run(argv),
            Command::Partition => cmd::partition::run(argv),
            Command::Pseudo => cmd::pseudo::run(argv),
            Command::Rename => cmd::rename::run(argv),
            Command::Replace => cmd::replace::run(argv),
            Command::Reverse => cmd::reverse::run(argv),
            Command::Sample => cmd::sample::run(argv),
            Command::Search => cmd::search::run(argv),
            Command::SearchSet => cmd::searchset::run(argv),
            Command::Select => cmd::select::run(argv),
            Command::Slice => cmd::slice::run(argv),
            Command::Sort => cmd::sort::run(argv),
            Command::Split => cmd::split::run(argv),
            Command::Stats => cmd::stats::run(argv),
            Command::Table => cmd::table::run(argv),
            Command::Transpose => cmd::transpose::run(argv),
            #[cfg(feature = "foreach")]
            Command::ForEach => cmd::foreach::run(argv),
        }
    }
}

pub type CliResult<T> = Result<T, CliError>;

#[derive(Debug)]
pub enum CliError {
    Flag(docopt::Error),
    Csv(csv::Error),
    Io(io::Error),
    Other(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            CliError::Flag(ref e) => e.fmt(f),
            CliError::Csv(ref e) => e.fmt(f),
            CliError::Io(ref e) => e.fmt(f),
            CliError::Other(ref s) => f.write_str(&**s),
        }
    }
}

impl From<docopt::Error> for CliError {
    fn from(err: docopt::Error) -> CliError {
        CliError::Flag(err)
    }
}

impl From<csv::Error> for CliError {
    fn from(err: csv::Error) -> CliError {
        if !err.is_io_error() {
            return CliError::Csv(err);
        }
        match err.into_kind() {
            csv::ErrorKind::Io(v) => From::from(v),
            _ => unreachable!(),
        }
    }
}

impl From<io::Error> for CliError {
    fn from(err: io::Error) -> CliError {
        CliError::Io(err)
    }
}

impl From<String> for CliError {
    fn from(err: String) -> CliError {
        CliError::Other(err)
    }
}

impl<'a> From<&'a str> for CliError {
    fn from(err: &'a str) -> CliError {
        CliError::Other(err.to_owned())
    }
}

impl From<regex::Error> for CliError {
    fn from(err: regex::Error) -> CliError {
        CliError::Other(format!("{:?}", err))
    }
}
