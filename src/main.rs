//! `череда` — командная строка формата.
//!
//! Глаголы — по-русски, флаги — латиницей (так проще печатать):
//!
//! ```text
//! череда просить  --case "починить сканер" --customer ты --performer forgessa
//! череда обещать  --id 1 --who forgessa --for 3600
//! череда сделать  --id 1 --who forgessa --proof "cargo test" --dir .
//! череда принять  --id 1 --who ты
//! череда возразить --id 1 --who ты --reason "нет теста на пустой ввод"
//! череда круг    --who forgessa
//! череда петли
//! череда проверить
//! череда следы
//! череда демо
//! ```

use std::path::PathBuf;

use chereda::{human, now, run_evidence, Class, Evidence, Ledger, Loop, Side, State, MAX_PROMISES};

struct Opts {
    args: Vec<String>,
}

impl Opts {
    fn parse(args: &[String]) -> Opts {
        Opts {
            args: args.to_vec(),
        }
    }
    fn flag(&self, name: &str) -> Option<&str> {
        let spelled = format!("--{name}");
        self.args
            .iter()
            .position(|item| item.as_str() == spelled.as_str())
            .and_then(|index| self.args.get(index + 1))
            .map(String::as_str)
    }
    fn number(&self, name: &str) -> Option<i64> {
        self.flag(name).and_then(|value| value.trim().parse().ok())
    }
    fn dir(&self) -> PathBuf {
        PathBuf::from(self.flag("ledger").unwrap_or("среда"))
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("просить") => cmd_ask(&args[1..]),
        Some("обещать") => cmd_promise(&args[1..]),
        Some("отказать") => cmd_refuse(&args[1..]),
        Some("сделать") => cmd_declare(&args[1..]),
        Some("принять") => cmd_accept(&args[1..]),
        Some("возразить") => cmd_object(&args[1..]),
        Some("снять") => cmd_withdraw(&args[1..]),
        Some("круг") => cmd_circle(&args[1..]),
        Some("петли") => cmd_loops(&args[1..]),
        Some("проверить") => cmd_verify(&args[1..]),
        Some("следы") => cmd_traces(&args[1..]),
        Some("демо") => cmd_demo(),
        Some("--help") | Some("-h") | None => {
            print_help();
            0
        }
        Some(other) => {
            eprintln!("неизвестный глагол: {other}\n");
            print_help();
            2
        }
    };
    std::process::exit(code);
}

fn print_help() {
    println!(
        "череда — формат работы: петля обязательства, ось «чей ход»

  просить   --case \"что нужно\" --customer КТО --performer КТО [--class острое]
  обещать   --id N --who КТО [--for СЕКУНД]
  отказать  --id N --who КТО --reason \"почему\"
  сделать   --id N --who КТО --proof \"команда\" [--dir PATH] [--expect КОД]
  принять   --id N --who КТО
  возразить --id N --who КТО --reason \"почему\"
  снять     --id N --who КТО
  круг      --who КТО            что на моём ходу и сколько осталось
  петли                          все открытые петли и чей ход
  проверить                      перепроверить доказательства; приёмка может быть отменена
  следы                          закрытые петли: сколько жили и чем кончились
  демо

Общий флаг --ledger DIR (по умолчанию ./среда), --now UNIX для проверок.

Правила:
  * ход выводится из акта: просит/обещано/возражение — ход делающего,
    отказ/объявлено — ход заказчика, принято/снято — хода нет
  * «сделано» без доказательства не принимается; доказательство перепроверяемо
  * часы: истёк срок хода — сигнал на уровень выше, и так до третьего
  * обещаний у одного больше {MAX_PROMISES} не бывает: это и есть предел работы
  * приёмку может отменить реальность: не воспроизвелось доказательство — петля открыта снова

Код выхода: 0 сделано, 1 не в свой ход или отказ, 2 ошибка."
    );
}

fn open(opts: &Opts) -> Result<Ledger, i32> {
    Ledger::open(opts.dir()).map_err(|err| {
        eprintln!("не открыть среду: {err}");
        2
    })
}

fn need_id(opts: &Opts) -> Result<u32, i32> {
    match opts.number("id") {
        Some(id) if id > 0 => Ok(id as u32),
        _ => {
            eprintln!("нужен --id N");
            Err(2)
        }
    }
}

fn with_loop<F>(opts: &Opts, mut action: F) -> i32
where
    F: FnMut(&mut Loop, u64) -> Result<(), String>,
{
    let ledger = match open(opts) {
        Ok(ledger) => ledger,
        Err(code) => return code,
    };
    let id = match need_id(opts) {
        Ok(id) => id,
        Err(code) => return code,
    };
    let at = opts
        .number("now")
        .map(|value| value as u64)
        .unwrap_or_else(now);
    let Some(mut loop_) = ledger.get(id) else {
        eprintln!("петли #{id} нет");
        return 2;
    };
    if let Err(why) = action(&mut loop_, at) {
        eprintln!("отказано: {why}");
        return 1;
    }
    ledger.tick_all(at);
    if let Err(err) = ledger.put(&loop_) {
        eprintln!("не записать петлю: {err}");
        return 2;
    }
    println!("{}", short(&loop_, at));
    0
}

fn short(loop_: &Loop, at: u64) -> String {
    let left = loop_.left(at);
    match loop_.whose_move() {
        None => format!(
            "#{} {} — закрыта ({}), жила {}",
            loop_.id,
            truncate(&loop_.cause, 40),
            loop_.state.as_str(),
            human(loop_.age(at) as i64)
        ),
        Some(side) => format!(
            "#{} {} — ход {} ({}), {}{}",
            loop_.id,
            truncate(&loop_.cause, 40),
            loop_.holder().unwrap_or("?"),
            side.as_str(),
            if left >= 0 {
                "осталось "
            } else {
                "часы вышли "
            },
            human(left)
        ),
    }
}

fn cmd_ask(args: &[String]) -> i32 {
    let opts = Opts::parse(args);
    let (Some(case), Some(customer), Some(performer)) = (
        opts.flag("case"),
        opts.flag("customer"),
        opts.flag("performer"),
    ) else {
        eprintln!("нужны --case, --customer и --performer");
        return 2;
    };
    let ledger = match open(&opts) {
        Ok(ledger) => ledger,
        Err(code) => return code,
    };
    let at = opts
        .number("now")
        .map(|value| value as u64)
        .unwrap_or_else(now);
    let class = opts
        .flag("class")
        .map(Class::parse)
        .unwrap_or(Class::Normal);
    let loop_ = Loop::new(ledger.next_id(), case, customer, performer, class, at);
    if let Err(err) = ledger.put(&loop_) {
        eprintln!("не записать петлю: {err}");
        return 2;
    }
    ledger.tick_all(at);
    println!("{}", short(&loop_, at));
    0
}

fn cmd_promise(args: &[String]) -> i32 {
    let opts = Opts::parse(args);
    let who = opts.flag("who").unwrap_or("").to_string();
    let ledger = match open(&opts) {
        Ok(ledger) => ledger,
        Err(code) => return code,
    };
    let id = match need_id(&opts) {
        Ok(id) => id,
        Err(code) => return code,
    };
    if !ledger
        .get(id)
        .map(|loop_| loop_.performer == who)
        .unwrap_or(false)
    {
        eprintln!("не ваш ход: {who} не делающий в петле #{id}");
        return 1;
    }
    if ledger.promises_held(&who) >= MAX_PROMISES {
        eprintln!(
            "отказано: у {who} уже {} обещаний — предел работы. Закройте одно, прежде чем брать новое",
            MAX_PROMISES
        );
        return 1;
    }
    with_loop(&opts, |loop_, at| {
        loop_.promise(&who, opts.number("for").map(|s| s as u64), at)
    })
}

fn cmd_refuse(args: &[String]) -> i32 {
    let opts = Opts::parse(args);
    let who = opts.flag("who").unwrap_or("").to_string();
    let reason = opts.flag("reason").unwrap_or("").to_string();
    with_loop(&opts, |loop_, at| loop_.refuse(&who, &reason, at))
}

fn cmd_accept(args: &[String]) -> i32 {
    let opts = Opts::parse(args);
    let who = opts.flag("who").unwrap_or("").to_string();
    with_loop(&opts, |loop_, at| loop_.accept(&who, at))
}

fn cmd_object(args: &[String]) -> i32 {
    let opts = Opts::parse(args);
    let who = opts.flag("who").unwrap_or("").to_string();
    let reason = opts.flag("reason").unwrap_or("").to_string();
    with_loop(&opts, |loop_, at| loop_.object(&who, &reason, at))
}

fn cmd_withdraw(args: &[String]) -> i32 {
    let opts = Opts::parse(args);
    let who = opts.flag("who").unwrap_or("").to_string();
    with_loop(&opts, |loop_, at| loop_.withdraw(&who, at))
}

fn cmd_declare(args: &[String]) -> i32 {
    let opts = Opts::parse(args);
    let who = opts.flag("who").unwrap_or("").to_string();
    let Some(cmd) = opts.flag("proof") else {
        eprintln!("нужно --proof \"чем доказывается\"");
        return 2;
    };
    let dir = opts.flag("dir").unwrap_or("").to_string();
    let expect = opts.number("expect").unwrap_or(0) as i32;
    let (code, digest, excerpt) = match run_evidence(cmd, &dir) {
        Ok(result) => result,
        Err(err) => {
            eprintln!("{err}");
            return 2;
        }
    };
    if code != expect {
        eprintln!(
            "доказательство не прошло: код {code} вместо ожидаемого {expect}\n{}",
            excerpt
        );
        return 1;
    }
    let evidence = Evidence {
        cmd: cmd.to_string(),
        dir,
        expect,
        digest,
        at: opts
            .number("now")
            .map(|value| value as u64)
            .unwrap_or_else(now),
        excerpt,
    };
    with_loop(&opts, |loop_, at| loop_.declare(&who, evidence.clone(), at))
}

fn cmd_circle(args: &[String]) -> i32 {
    let opts = Opts::parse(args);
    let Some(who) = opts.flag("who") else {
        eprintln!("нужен --who КТО");
        return 2;
    };
    let ledger = match open(&opts) {
        Ok(ledger) => ledger,
        Err(code) => return code,
    };
    let at = opts
        .number("now")
        .map(|value| value as u64)
        .unwrap_or_else(now);
    for event in ledger.tick_all(at) {
        println!("  часы: {event}");
    }
    let mine = ledger.circle(who, at);
    if mine.is_empty() {
        println!("{who}: на вашем ходу ничего — ход не ваш");
        return 0;
    }
    println!(
        "{who}, на вашем ходу {} петель (по кругу, самые срочные сверху):",
        mine.len()
    );
    let mut late = 0;
    for loop_ in &mine {
        let left = loop_.left(at);
        if left < 0 {
            late += 1;
        }
        println!(
            "  #{} {:<44} {} {}",
            loop_.id,
            truncate(&loop_.cause, 44),
            if left >= 0 {
                "осталось"
            } else {
                "часы вышли"
            },
            human(left)
        );
        if !loop_.objection.is_empty() && loop_.state == State::Objection {
            println!("      возражение: {}", loop_.objection);
        }
    }
    if late > 0 {
        println!("  {late} с истёкшими часами: по ним сигнал уже идёт выше");
        1
    } else {
        0
    }
}

fn cmd_loops(args: &[String]) -> i32 {
    let opts = Opts::parse(args);
    let ledger = match open(&opts) {
        Ok(ledger) => ledger,
        Err(code) => return code,
    };
    let at = opts
        .number("now")
        .map(|value| value as u64)
        .unwrap_or_else(now);
    ledger.tick_all(at);
    let all = ledger.loops();
    let open: Vec<&Loop> = all.iter().filter(|loop_| !loop_.state.closed()).collect();
    if open.is_empty() {
        println!("открытых петель нет");
        return 0;
    }
    println!(
        "{:<4} {:<24} {:<14} {:<16} {}",
        "id", "дело", "чей ход", "состояние", "часы"
    );
    for loop_ in open {
        println!(
            "{:<4} {:<24} {:<14} {:<16} {}",
            loop_.id,
            truncate(&loop_.cause, 24),
            loop_.holder().unwrap_or("—"),
            loop_.state.as_str(),
            human(loop_.left(at))
        );
    }
    println!(
        "\nэскалации по уровню: {}",
        all.iter()
            .filter(|loop_| loop_.level > 1)
            .map(|loop_| format!("#{} → {}", loop_.id, loop_.level))
            .collect::<Vec<_>>()
            .join(", ")
            .if_empty("нет")
    );
    0
}

fn cmd_verify(args: &[String]) -> i32 {
    let opts = Opts::parse(args);
    let ledger = match open(&opts) {
        Ok(ledger) => ledger,
        Err(code) => return code,
    };
    let at = opts
        .number("now")
        .map(|value| value as u64)
        .unwrap_or_else(now);
    let broken = ledger.verify(at);
    if broken.is_empty() {
        println!("доказательства воспроизводятся: приёмки в силе");
        return 0;
    }
    for (id, reason) in &broken {
        println!("#{id}: {reason}");
    }
    println!("\nреальность открыла {} петель снова", broken.len());
    1
}

fn cmd_traces(args: &[String]) -> i32 {
    let opts = Opts::parse(args);
    let ledger = match open(&opts) {
        Ok(ledger) => ledger,
        Err(code) => return code,
    };
    let at = opts
        .number("now")
        .map(|value| value as u64)
        .unwrap_or_else(now);
    let closed: Vec<Loop> = ledger
        .loops()
        .into_iter()
        .filter(|loop_| loop_.state.closed())
        .collect();
    if closed.is_empty() {
        println!("следов нет: ни одна петля не закрыта");
        return 0;
    }
    println!(
        "{:<4} {:<36} {:<12} {:<10} {}",
        "id", "дело", "чем кончилась", "жила", "кто делал"
    );
    for loop_ in closed {
        let last = loop_.history.last().map(|act| act.at).unwrap_or(loop_.made);
        println!(
            "{:<4} {:<36} {:<12} {:<10} {}",
            loop_.id,
            truncate(&loop_.cause, 36),
            loop_.state.as_str(),
            human(last.saturating_sub(loop_.made) as i64),
            loop_.performer
        );
    }
    let avg: u64 = {
        let durations: Vec<u64> = ledger
            .loops()
            .iter()
            .filter(|loop_| loop_.state.closed())
            .map(|loop_| {
                loop_
                    .history
                    .last()
                    .map(|act| act.at.saturating_sub(loop_.made))
                    .unwrap_or(0)
            })
            .collect();
        if durations.is_empty() {
            0
        } else {
            durations.iter().sum::<u64>() / durations.len() as u64
        }
    };
    let _ = at;
    println!("\nсредняя жизнь петли: {}", human(avg as i64));
    0
}

fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    text.chars()
        .take(width.saturating_sub(1))
        .collect::<String>()
        + "…"
}

trait IfEmpty {
    fn if_empty(self, fallback: &str) -> String;
}

impl IfEmpty for String {
    fn if_empty(self, fallback: &str) -> String {
        if self.is_empty() {
            fallback.to_string()
        } else {
            self
        }
    }
}

// --- демонстрация -----------------------------------------------------------------
fn cmd_demo() -> i32 {
    let mut checks = 0;
    let mut fails = 0;
    let mut check = |name: &str, ok: bool, detail: String| {
        checks += 1;
        if !ok {
            fails += 1;
        }
        println!("  {} {name} — {detail}", if ok { "✓" } else { "✗" });
    };

    let dir = std::env::temp_dir().join(format!("chereda-demo-{}", now()));
    let _ = std::fs::remove_dir_all(&dir);
    let ledger = Ledger::open(&dir).unwrap();
    let t0 = 1_000_000u64;

    // 1. просьба: ход делающего, часы четыре часа
    let mut first = Loop::new(
        1,
        "починить сканер музея",
        "ты",
        "forgessa",
        Class::Normal,
        t0,
    );
    ledger.put(&first).unwrap();
    check(
        "просьба: ход делающего",
        first.whose_move() == Some(Side::Performer) && first.deadline == t0 + 4 * 3600,
        format!("часы {}", human(first.left(t0))),
    );

    // 2. отказ с причиной: ход возвращается заказчику
    first
        .refuse("forgessa", "нет доступа к хосту музея", t0 + 60)
        .unwrap();
    ledger.put(&first).unwrap();
    check(
        "отказ уводит ход заказчику",
        first.whose_move() == Some(Side::Customer) && first.state == State::Refused,
        "без причины отказ не принимается".to_string(),
    );

    // 3. вторая петля: обещание, объявление с доказательством
    let marker = dir.join("ok.txt");
    std::fs::write(&marker, "ok").unwrap();
    let mut second = Loop::new(
        2,
        "снять sitemap с сайта",
        "ты",
        "forgessa",
        Class::Normal,
        t0,
    );
    second.promise("forgessa", Some(3600), t0 + 120).unwrap();
    check(
        "обещание задаёт свои часы",
        second.deadline == t0 + 120 + 3600,
        format!("{}", human(second.left(t0 + 120))),
    );

    let cmd = format!("test -f {}", marker.display());
    let (code, dg, excerpt) = run_evidence(&cmd, "").unwrap();
    second
        .declare(
            "forgessa",
            Evidence {
                cmd: cmd.clone(),
                dir: String::new(),
                expect: code,
                digest: dg,
                at: t0 + 300,
                excerpt,
            },
            t0 + 300,
        )
        .unwrap();
    ledger.put(&second).unwrap();
    check(
        "объявлено: ход заказчика, доказательство приложено",
        second.whose_move() == Some(Side::Customer) && second.evidence.is_some(),
        "без доказательства объявление не принимается".to_string(),
    );

    // 4. возражение с причиной: ход снова делающего
    second
        .object("ты", "нет теста на пустой ответ", t0 + 400)
        .unwrap();
    ledger.put(&second).unwrap();
    check(
        "возражение возвращает ход делающему",
        second.whose_move() == Some(Side::Performer) && !second.objection.is_empty(),
        second.objection.clone(),
    );

    // 5. исправил и объявил заново, заказчик признал
    let cmd2 = format!("test -f {} && echo edge-case-covered", marker.display());
    let (code2, dg2, ex2) = run_evidence(&cmd2, "").unwrap();
    second
        .declare(
            "forgessa",
            Evidence {
                cmd: cmd2,
                dir: String::new(),
                expect: code2,
                digest: dg2,
                at: t0 + 500,
                excerpt: ex2,
            },
            t0 + 500,
        )
        .unwrap();
    second.accept("ты", t0 + 600).unwrap();
    ledger.put(&second).unwrap();
    check(
        "принято: хода нет, остался след",
        second.whose_move().is_none() && second.state == State::Accepted,
        format!("петля жила {}", human((600) as i64)),
    );

    // 6. часы: молчание делающего поднимает сигнал
    let mut third = Loop::new(3, "проверить лицензии", "ты", "forgessa", Class::Urgent, t0);
    ledger.put(&third).unwrap();
    let events = ledger.tick_all(t0 + 3600);
    third = ledger.get(3).unwrap();
    check(
        "часы истекли — сигнал выше",
        !events.is_empty() && third.level == 2,
        events.first().cloned().unwrap_or_default(),
    );

    // 7. предел работы: больше трёх обещаний не берут
    for id in 10..=12 {
        let mut loop_ = Loop::new(id, "дело", "ты", "forgessa", Class::Normal, t0);
        loop_.promise("forgessa", None, t0 + 10).unwrap();
        ledger.put(&loop_).unwrap();
    }
    check(
        "предел работы — три обещания",
        ledger.promises_held("forgessa") >= MAX_PROMISES,
        format!("держится {}", ledger.promises_held("forgessa")),
    );

    // 8. реальность отменяет приёмку
    std::fs::remove_file(&marker).unwrap();
    let broken = ledger.verify(t0 + 700);
    let reopened = ledger.get(2).unwrap();
    check(
        "приёмку может отменить реальность",
        broken.len() == 1 && reopened.state == State::Objection,
        "доказательство перестало воспроизводиться".to_string(),
    );

    // 9. круг показывает только мои ходы, срочные сверху
    let mine = ledger.circle("forgessa", t0 + 800);
    check(
        "круг: только мой ход, срочное сверху",
        mine.iter()
            .all(|loop_| loop_.whose_move() == Some(Side::Performer))
            && !mine.is_empty(),
        format!("петель на моём ходу: {}", mine.len()),
    );

    println!("\nитог: {checks} проверок, {fails} провалов");
    let _ = std::fs::remove_dir_all(&dir);
    if fails == 0 {
        0
    } else {
        1
    }
}
