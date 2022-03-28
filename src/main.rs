extern crate skim;
use chrono::NaiveDateTime;
use enum_map::{enum_map, Enum};
use rusqlite::{Connection, Result};
use skim::prelude::*;
use std::env;
use std::thread;
use std::time::SystemTime;


#[derive(PartialEq, Enum, Copy, Clone)]
enum Location {
    Session,
    Directory,
    Machine,
    Everywhere,
}

#[derive(Debug)]
struct History {
    id: i64,
    cmd: String,
    start: u64,
    exit_status: Option<i64>,
    duration: Option<i64>,
    count: i64,
    session: i64,
    host: String,
    dir: String,
}

impl SkimItem for History {
    fn text(&self) -> Cow<str> {
        let now = SystemTime::now();
        let now_secs = now
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let seconds_since_midnight = now_secs % (24 * 3600);
        let starttime = NaiveDateTime::from_timestamp(self.start as i64, 0);
        let mut dateinfo = String::from("");
        if self.start > (now_secs - seconds_since_midnight) {
            dateinfo.push_str(&format!("{}", starttime.format("%H:%M")));
        } else {
            dateinfo.push_str(&format!("{}", starttime.format(&get_date_format())));
        }

        let information = format!("{:10} {}", dateinfo, self.cmd);
        Cow::Owned(information)
    }

    fn preview(&self, _context: PreviewContext) -> ItemPreview {
        let starttime = NaiveDateTime::from_timestamp(self.start as i64, 0);

        let mut timeformat = String::from("");
        timeformat.push_str(&get_date_format());
        timeformat.push_str(" %H:%M");

        let mut information = String::from(format!("\x1b[1mDetails for {}\x1b[0m\n\n", self.id));

        let mut tformat = |name: &str, value: &str| {
            information.push_str(&format!("\x1b[1m{:20}\x1b[0m{}\n", name, value));
        };

        let duration = || -> String {
            if self.duration.is_some() {
                format!("{}", self.duration.unwrap())
            } else {
                "<NONE>".to_string()
            }
        }();
        tformat("Runtime", &duration);
        tformat("Host", &self.host);
        tformat("Executed", &self.count.to_string());
        tformat("Directory", &self.dir);
        let status = || -> String {
            if self.exit_status.is_some() {
                format!("{}", self.exit_status.unwrap())
            } else {
                "<NONE>".to_string()
            }
        }();
        tformat("Exit Status", &status);
        tformat("Session", &self.session.to_string());
        tformat("Start Time", &starttime.format(&timeformat).to_string());
        information.push_str(&format!(
            "\x1b[1m{:20}\x1b[0m\n\n{}\n",
            "Command", &self.cmd
        ));
        ItemPreview::AnsiText(information)
    }
}

/// Get the default (which is non us! or the us date format)
/// - [ ] Read from locale to determine default
fn get_date_format() -> String {
    let key = "HISTDB_FZF_FORCE_DATE_FORMAT";
    let forced_dateformat = env::var(key).unwrap_or("non-us".to_string()).to_lowercase();

    if forced_dateformat == "us" {
        return "%m/%d/%Y".to_string();
    } else {
        return "%d/%m/%Y".to_string();
    }
}

/// Get the histdb file from the environment
fn get_histdb_database() -> String {
    let key = "HISTDB_FILE";
    let db_file = env::var(key).unwrap_or(String::from(""));
    return db_file.to_string();
}

/// Get the histdb session from the environment
fn get_current_session_id() -> String {
    let key = "HISTDB_SESSION";
    let session_id = env::var(key).unwrap_or(String::from(""));
    return session_id.to_string();
}

/// Get the current working directory
fn get_current_dir() -> String {
    let current_dir = env::current_dir().unwrap();
    let cdir_string = current_dir.to_str().unwrap();
    return cdir_string.to_string();
}

/// Get the current histdb host from the environment
fn get_current_host() -> String {
    let mut host = env::var("HISTDB_HOST").unwrap_or(String::from(""));
    if host.starts_with("'") && host.ends_with("'") {
        host = host[1..host.len() - 1].to_string()
    }
    return host.to_string();
}

fn prepare_entries(location: &Location, grouped: bool, tx_item: SkimItemSender) {
    let conn_res = Connection::open(get_histdb_database());
    if conn_res.is_err() {
        return;
    }
    let conn = conn_res.unwrap();
    let s = build_query_string(&location, grouped);

    let stmt_result = conn.prepare(&s);
    if stmt_result.is_err() {
        return;
    }
    let mut stmt = stmt_result.unwrap();

    let cats = stmt.query_map([], |row| {
        Ok(History {
            id: row.get(0)?,
            cmd: row.get(1)?,
            start: row.get(2)?,
            exit_status: row.get(3)?,
            duration: row.get(4)?,
            count: row.get(5)?,
            session: row.get(6)?,
            host: row.get(7)?,
            dir: row.get(8)?,
        })
    });
    for person in cats.unwrap() {
        if person.is_ok() {
            let x = person.unwrap();
            let _ = tx_item.send(Arc::new(x));
        }
    }
    drop(tx_item);
}


fn show_history(thequery: String) -> Result<String> {
    let mut location = Location::Session;
    let mut grouped = true;
    let mut query = thequery;
    if get_current_session_id() == "" {
        location = Location::Directory;
    }
    loop {
        let map = enum_map! {
            Location::Session => "Session location history",
            Location::Directory => "Directory location history",
            Location::Machine => "Machine location history",
            Location::Everywhere => "Everywhere",
        };
        let extra_info = |theloc: &Location| -> String {
            return match theloc {
                Location::Session => get_current_session_id(),
                Location::Directory => get_current_dir(),
                Location::Machine => get_current_host(),
                _ => String::from(""),
            };
        }(&location);

        let title = format!(
            "{} {}\n{}\n―――――――――――――――――――――――――",
            &map[location.clone()],
            &extra_info,
            "F1: Session, F2: Directory, F3: Host, F4: Everywhere -- F5: Toggle group"
        );


        let options = SkimOptionsBuilder::default()
            .height(Some("100%"))
            .multi(false)
            .reverse(true)
            .prompt(Some("history >>"))
            .query(Some(&query))
            .bind(vec![
                "f1:abort",
                "f2:abort",
                "f3:abort",
                "f4:abort",
                "f5:abort",
                "ctrl-r:abort",
            ])
            .header(Some(&title))
            .preview(Some("")) // preview should be specified to enable preview window
            .nosort(true)
            .build()
            .unwrap();

        let (tx_item, rx_item): (SkimItemSender, SkimItemReceiver) = unbounded();

        let handle = thread::spawn(move || {
            prepare_entries(&location, grouped, tx_item);
        });

        let selected_items = Skim::run_with(&options, Some(rx_item));

        handle.join().unwrap();

        if selected_items.is_some() {
            let sel = selected_items.unwrap();
            query = sel.query;
            match sel.final_key {
                Key::ESC | Key::Ctrl('c') | Key::Ctrl('d') | Key::Ctrl('z') => {
                    std::process::exit(1);
                }
                Key::Enter => {
                    return Ok(format!(
                        "{}",
                        ((*sel.selected_items[0]).as_any().downcast_ref::<History>())
                            .unwrap()
                            .cmd
                    ))
                }
                Key::F(1) => {
                    location = Location::Session;
                }
                Key::F(2) => {
                    location = Location::Directory;
                }
                Key::F(3) => {
                    location = Location::Machine;
                }
                Key::F(4) => {
                    location = Location::Everywhere;
                }
                Key::F(5) => {
                    grouped = !grouped;
                }
                Key::Ctrl('r') => {
                    location = match location {
                        Location::Session => Location::Directory,
                        Location::Directory => Location::Machine,
                        Location::Machine => Location::Everywhere,
                        Location::Everywhere => Location::Session,
                    };
                }
                _ => (),
            };
        }
    }
}

fn main() -> Result<()> {
    let _conn = Connection::open(get_histdb_database())?;

    let args: Vec<String> = env::args().collect();
    let query = |args: Vec<String>| -> String {
        if args.len() > 1 {
            return args[1].to_string();
        }
        return "".to_string();
    }(args);

    let result = show_history(query);
    if result.is_ok() {
        println!("{}", result.ok().unwrap());
    } else {
        eprintln!("{}", result.err().unwrap());
        std::process::exit(1);
    }

    Ok(())
}

fn build_query_string(theloc: &Location, grouped: bool) -> String {
    let mut query = String::from(" select history.id, commands.argv, ");
    if !grouped {
        query.push_str("start_time")
    } else {
        query.push_str("max(start_time)")
    }
    query.push_str(" as max_start, exit_status, duration, ");
    if !grouped {
        query.push_str("1")
    } else {
        query.push_str("count()")
    }
    query.push_str(" as count, history.session, places.host, places.dir ");
    query.push_str(" from history ");
    query.push_str("
        left join commands on history.command_id = commands.id
        left join places on history.place_id = places.id",
    );
    match theloc {
        Location::Session | Location::Directory | Location::Machine => {
            query.push_str(" where ");
        }
        _ => {}
    };
    match theloc {
        Location::Session => {
            query.push_str(&format!(" session in ({}) and ", &get_current_session_id()))
        }

        Location::Directory => {
            query.push_str(&format!(" (places.dir like '{}') and ", &get_current_dir()))
        }

        _ => {}
    };
    match theloc {
        Location::Session | Location::Directory | Location::Machine => {
            query.push_str(&format!(" places.host='{}'", &get_current_host()));
        }
        _ => {}
    };
    if grouped {
        query.push_str(" group by history.command_id, history.place_id");
    }
    query.push_str( " order by max_start desc");
    return query;
}
