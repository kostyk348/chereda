//! Череда: формат работы, где единица — петля обязательства, а ось — **чей ход**.
//!
//! Не карточка со статусом, а разговор двоих: заказчик просит, делающий обещает,
//! объявляет сделанное (с доказательством), заказчик признаёт или возражает.
//! Состояния — это не колонки, а следы речевых актов (Флорес), и из них однозначно
//! выводится единственное, что имеет значение: **чей ход**.
//!
//! Что здесь от кого:
//!
//! * **Флорес** (он же позвал Бира в Чили) — петля «просьба → обещание → объявление →
//!   признание» и то, что «сделано» и «принято» — два разных акта.
//! * **Бир** — часы: у каждого хода есть срок; истёк — сигнал идёт на уровень выше.
//! * **Русский лад** — возражение не голосуется, а снимается; молчание заказчика
//!   НЕ означает согласия (в отличие от круга решений: тут цена ошибки другая).
//! * **Гастев** — «ни одного задания без измерений»: объявление без доказательства
//!   не принимается, а доказательство перепроверяемо.
//! * **Макаренко** — нет постоянного владельца работы: петля живёт, пока двоим есть
//!   что сказать; закрытая петля оставляет след, а не «карточку в архиве».
//! * **Фёдоров** — закрытие не вечно: если доказательство перестало воспроизводиться,
//!   реальность отменяет приёмку и петля открывается снова.
//!
//! Чего здесь нет: оценок, приоритетов, бэклога, статусов, колонок, WIP-лимита
//! в колонке. Ограничение работы выводится из физики: обещаний у одного делающего
//! не может быть больше, чем он держит (по умолчанию три петли).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Сколько петель один делающий может держать в состоянии «обещано».
pub const MAX_PROMISES: usize = 3;

/// Выше третьего уровня сигнал не идёт: дальше это уже не эскалация, а крик.
pub const MAX_LEVEL: u32 = 3;

pub const LEVELS: [&str; 3] = ["свой контур", "уровень выше", "владелец системы"];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Class {
    /// Обычное дело.
    Normal,
    /// Острое: часы короче.
    Urgent,
}

impl Class {
    pub fn as_str(&self) -> &'static str {
        match self {
            Class::Normal => "обычное",
            Class::Urgent => "острое",
        }
    }
    pub fn parse(text: &str) -> Class {
        if text.starts_with("ост") {
            Class::Urgent
        } else {
            Class::Normal
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum State {
    /// Просьба сделана, слово за делающим.
    Asked,
    /// Делающий отказался: слово за заказчиком (снять или перепоручить).
    Refused,
    /// Делающий обещал и взял.
    Promised,
    /// Объявил сделанное с доказательством: слово за заказчиком.
    Declared,
    /// Заказчик возразил: слово за делающим.
    Objection,
    /// Заказчик признал: петля закрыта, остался след.
    Accepted,
    /// Заказчик снял просьбу: петля закрыта.
    Withdrawn,
}

impl State {
    pub fn as_str(&self) -> &'static str {
        match self {
            State::Asked => "просит",
            State::Refused => "отказ",
            State::Promised => "обещано",
            State::Declared => "объявлено",
            State::Objection => "возражение",
            State::Accepted => "принято",
            State::Withdrawn => "снято",
        }
    }
    pub fn parse(text: &str) -> Option<State> {
        Some(match text {
            "просит" => State::Asked,
            "отказ" => State::Refused,
            "обещано" => State::Promised,
            "объявлено" => State::Declared,
            "возражение" => State::Objection,
            "принято" => State::Accepted,
            "снято" => State::Withdrawn,
            _ => return None,
        })
    }
    pub fn closed(&self) -> bool {
        matches!(self, State::Accepted | State::Withdrawn)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Side {
    Customer,
    Performer,
}

impl Side {
    pub fn as_str(&self) -> &'static str {
        match self {
            Side::Customer => "заказчик",
            Side::Performer => "делающий",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Act {
    pub at: u64,
    pub who: String,
    pub what: String,
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct Evidence {
    pub cmd: String,
    pub dir: String,
    pub expect: i32,
    pub digest: String,
    pub at: u64,
    pub excerpt: String,
}

#[derive(Debug, Clone)]
pub struct Loop {
    pub id: u32,
    /// Одна фраза: что нужно и зачем. Без «и ещё».
    pub cause: String,
    pub customer: String,
    pub performer: String,
    pub class: Class,
    pub state: State,
    pub made: u64,
    pub deadline: u64,
    pub level: u32,
    pub history: Vec<Act>,
    pub evidence: Option<Evidence>,
    pub objection: String,
}

impl Loop {
    pub fn new(
        id: u32,
        cause: &str,
        customer: &str,
        performer: &str,
        class: Class,
        at: u64,
    ) -> Loop {
        Loop {
            id,
            cause: cause.to_string(),
            customer: customer.to_string(),
            performer: performer.to_string(),
            class,
            state: State::Asked,
            made: at,
            deadline: at + clock_for(State::Asked, class),
            level: 1,
            history: vec![Act {
                at,
                who: customer.to_string(),
                what: "просьба".to_string(),
                note: String::new(),
            }],
            evidence: None,
            objection: String::new(),
        }
    }

    /// Единственная ось формата.
    pub fn whose_move(&self) -> Option<Side> {
        match self.state {
            State::Asked | State::Promised | State::Objection => Some(Side::Performer),
            State::Refused | State::Declared => Some(Side::Customer),
            State::Accepted | State::Withdrawn => None,
        }
    }

    pub fn holder(&self) -> Option<&str> {
        match self.whose_move() {
            Some(Side::Customer) => Some(self.customer.as_str()),
            Some(Side::Performer) => Some(self.performer.as_str()),
            None => None,
        }
    }

    pub fn left(&self, at: u64) -> i64 {
        self.deadline as i64 - at as i64
    }

    /// Сколько петля живёт от просьбы до сего момента.
    pub fn age(&self, at: u64) -> u64 {
        at.saturating_sub(self.made)
    }

    fn act(&mut self, at: u64, who: &str, what: &str, note: &str) {
        self.history.push(Act {
            at,
            who: who.to_string(),
            what: what.to_string(),
            note: note.to_string(),
        });
    }

    fn advance(&mut self, state: State, at: u64) {
        self.state = state;
        self.deadline = at + clock_for(state, self.class);
    }

    // --- речевые акты -----------------------------------------------------------
    pub fn refuse(&mut self, who: &str, reason: &str, at: u64) -> Result<(), String> {
        self.expect(Side::Performer, who)?;
        if reason.trim().is_empty() {
            return Err("отказ без причины — это не отказ, а молчание".to_string());
        }
        self.act(at, who, "отказ", reason);
        self.advance(State::Refused, at);
        Ok(())
    }

    pub fn promise(&mut self, who: &str, for_secs: Option<u64>, at: u64) -> Result<(), String> {
        self.expect(Side::Performer, who)?;
        let promise_clock = for_secs.unwrap_or(clock_for(State::Promised, self.class));
        let cap = clock_for(State::Promised, self.class);
        let given = promise_clock.min(cap);
        self.act(at, who, "обещание", &format!("на {}", human(given as i64)));
        self.state = State::Promised;
        self.deadline = at + given;
        Ok(())
    }

    pub fn declare(&mut self, who: &str, evidence: Evidence, at: u64) -> Result<(), String> {
        self.expect(Side::Performer, who)?;
        if evidence.digest.is_empty() {
            return Err("объявление без доказательства не принимается".to_string());
        }
        self.act(
            at,
            who,
            "объявил",
            &format!("доказательство: {}", evidence.cmd),
        );
        self.evidence = Some(evidence);
        self.advance(State::Declared, at);
        Ok(())
    }

    pub fn accept(&mut self, who: &str, at: u64) -> Result<(), String> {
        self.expect(Side::Customer, who)?;
        self.act(at, who, "принял", "");
        self.advance(State::Accepted, at);
        Ok(())
    }

    pub fn object(&mut self, who: &str, reason: &str, at: u64) -> Result<(), String> {
        self.expect(Side::Customer, who)?;
        if reason.trim().is_empty() {
            return Err("возражение без причины не принимается".to_string());
        }
        self.act(at, who, "возразил", reason);
        self.objection = reason.to_string();
        self.advance(State::Objection, at);
        Ok(())
    }

    pub fn withdraw(&mut self, who: &str, at: u64) -> Result<(), String> {
        self.expect(Side::Customer, who)?;
        self.act(at, who, "снял", "");
        self.advance(State::Withdrawn, at);
        Ok(())
    }

    /// Реальность отменяет приёмку: доказательство перестало воспроизводиться.
    pub fn reopen(&mut self, reason: &str, at: u64) -> Result<(), String> {
        if self.state.closed() {
            self.act(at, "реальность", "открыто снова", reason);
            self.state = State::Objection;
            self.objection = reason.to_string();
            self.deadline = at + clock_for(State::Objection, self.class);
            return Ok(());
        }
        Err(format!(
            "петля #{:?} не закрыта, переоткрывать нечего",
            self.id
        ))
    }

    fn expect(&self, side: Side, who: &str) -> Result<(), String> {
        match self.whose_move() {
            None => Err(format!("петля #{} уже закрыта", self.id)),
            Some(actual) => {
                if actual != side {
                    return Err(format!(
                        "сейчас ход {} — говорите вы ({who}) не в свой ход",
                        actual.as_str()
                    ));
                }
                if self.holder() != Some(who) {
                    return Err(format!(
                        "ход {} принадлежит {}, а не {who}",
                        Side::Customer.as_str(),
                        self.holder().unwrap_or("?")
                    ));
                }
                Ok(())
            }
        }
    }

    /// Оборот часов: истёк срок — сигнал на уровень выше, часы заново.
    pub fn tick(&mut self, at: u64) -> Option<String> {
        if self.state.closed() || at < self.deadline {
            return None;
        }
        let before = self.level;
        if self.level < MAX_LEVEL {
            self.level += 1;
        }
        self.deadline = at + clock_for(self.state, self.class);
        self.act(
            at,
            "часы",
            "эскалация",
            &format!(
                "{}: {} → {}",
                if before == self.level {
                    "повтор"
                } else {
                    "уровень"
                },
                LEVELS[(before.min(MAX_LEVEL) as usize) - 1],
                LEVELS[(self.level as usize) - 1]
            ),
        );
        Some(format!(
            "#{} {} не ответил за {} → {}",
            self.id,
            self.holder().unwrap_or("никто"),
            human(clock_for(self.state, self.class) as i64),
            LEVELS[(self.level as usize) - 1]
        ))
    }

    pub fn render(&self) -> String {
        let mut out = format!(
            "id={}\ncase={}\ncustomer={}\nperformer={}\nclass={}\nstate={}\nmade={}\ndeadline={}\nlevel={}\nobjection={}\n",
            self.id,
            self.cause,
            self.customer,
            self.performer,
            self.class.as_str(),
            self.state.as_str(),
            self.made,
            self.deadline,
            self.level,
            self.objection
        );
        if let Some(evidence) = &self.evidence {
            out.push_str(&format!(
                "proof={}|{}|{}|{}|{}\n",
                evidence.cmd, evidence.dir, evidence.expect, evidence.digest, evidence.at
            ));
        }
        for act in &self.history {
            out.push_str(&format!(
                "act={}|{}|{}|{}\n",
                act.at, act.who, act.what, act.note
            ));
        }
        out
    }

    pub fn parse(text: &str) -> Option<Loop> {
        let mut id = 0;
        let mut cause = String::new();
        let mut customer = String::new();
        let mut performer = String::new();
        let mut class = Class::Normal;
        let mut state = State::Asked;
        let mut made = 0;
        let mut deadline = 0;
        let mut level = 1;
        let mut objection = String::new();
        let mut evidence = None;
        let mut history = Vec::new();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key.trim() {
                "id" => id = value.trim().parse().unwrap_or(0),
                "case" => cause = value.trim().to_string(),
                "customer" => customer = value.trim().to_string(),
                "performer" => performer = value.trim().to_string(),
                "class" => class = Class::parse(value.trim()),
                "state" => state = State::parse(value.trim())?,
                "made" => made = value.trim().parse().unwrap_or(0),
                "deadline" => deadline = value.trim().parse().unwrap_or(0),
                "level" => level = value.trim().parse().unwrap_or(1),
                "objection" => objection = value.trim().to_string(),
                "proof" => {
                    let parts: Vec<&str> = value.split('|').collect();
                    if parts.len() >= 5 {
                        evidence = Some(Evidence {
                            cmd: parts[0].to_string(),
                            dir: parts[1].to_string(),
                            expect: parts[2].parse().unwrap_or(0),
                            digest: parts[3].to_string(),
                            at: parts[4].parse().unwrap_or(0),
                            excerpt: String::new(),
                        });
                    }
                }
                "act" => {
                    let parts: Vec<&str> = value.split('|').collect();
                    if parts.len() >= 4 {
                        history.push(Act {
                            at: parts[0].parse().unwrap_or(0),
                            who: parts[1].to_string(),
                            what: parts[2].to_string(),
                            note: parts[3..].join("|"),
                        });
                    }
                }
                _ => {}
            }
        }
        if id == 0 || cause.is_empty() {
            return None;
        }
        Some(Loop {
            id,
            cause,
            customer,
            performer,
            class,
            state,
            made,
            deadline,
            level,
            history,
            evidence,
            objection,
        })
    }
}

/// Часы по состоянию и классу дела.
pub fn clock_for(state: State, class: Class) -> u64 {
    let urgent = class == Class::Urgent;
    match state {
        State::Asked => {
            if urgent {
                3600
            } else {
                4 * 3600
            }
        }
        State::Promised => {
            if urgent {
                4 * 3600
            } else {
                24 * 3600
            }
        }
        State::Declared => {
            if urgent {
                2 * 3600
            } else {
                24 * 3600
            }
        }
        State::Objection => {
            if urgent {
                2 * 3600
            } else {
                8 * 3600
            }
        }
        State::Refused => {
            if urgent {
                8 * 3600
            } else {
                24 * 3600
            }
        }
        State::Accepted | State::Withdrawn => 0,
    }
}

/// Отпечаток вывода: FNV-1a 64, не криптография — только сравнение.
pub fn digest(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Убрать из вывода то, что меняется от прогона к прогону.
pub fn normalize(output: &str) -> String {
    let mut out = String::with_capacity(output.len());
    for line in output.lines() {
        let mut first = true;
        for token in line.split_whitespace() {
            if !first {
                out.push(' ');
            }
            first = false;
            out.push_str(&stable_token(token));
        }
        out.push('\n');
    }
    out
}

fn stable_token(token: &str) -> String {
    let trimmed = token.trim_end_matches([',', ';', ')', ':']);
    let suffix = &token[trimmed.len()..];
    if let Some(number) = trimmed.strip_suffix('s') {
        if !number.is_empty() && number.parse::<f64>().is_ok() {
            return format!("<t>{suffix}");
        }
    }
    token.to_string()
}

/// Прогон доказательства: единственное место, где что-то исполняется.
pub fn run_evidence(cmd: &str, dir: &str) -> Result<(i32, String, String), String> {
    let mut command = Command::new("sh");
    command.arg("-c").arg(cmd);
    if !dir.is_empty() {
        command.current_dir(dir);
    }
    let output = command
        .output()
        .map_err(|err| format!("не запустить «{cmd}»: {err}"))?;
    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    let errors = String::from_utf8_lossy(&output.stderr).to_string();
    if !errors.trim().is_empty() {
        text.push_str("\n--- stderr ---\n");
        text.push_str(&errors);
    }
    let normalized = normalize(&text);
    let tail: Vec<&str> = normalized.lines().rev().take(3).collect();
    let excerpt = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
    Ok((
        output.status.code().unwrap_or(-1),
        digest(&normalized),
        excerpt,
    ))
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn human(secs: i64) -> String {
    let value = secs.unsigned_abs();
    let text = if value >= 86_400 {
        format!("{}д", value / 86_400)
    } else if value >= 3600 {
        format!("{}ч", value / 3600)
    } else if value >= 60 {
        format!("{}м", value / 60)
    } else {
        format!("{value}с")
    };
    if secs < 0 {
        format!("−{text}")
    } else {
        text
    }
}

/// Реестр петель: каталог `loops/`, по файлу на петлю. Никакого сервера.
pub struct Ledger {
    root: PathBuf,
}

impl Ledger {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Ledger> {
        let root = path.as_ref().to_path_buf();
        fs::create_dir_all(root.join("loops"))?;
        Ok(Ledger { root })
    }

    fn dir(&self) -> PathBuf {
        self.root.join("loops")
    }

    pub fn loops(&self) -> Vec<Loop> {
        let mut out = Vec::new();
        let Ok(entries) = fs::read_dir(self.dir()) else {
            return out;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("loop") {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            if let Some(loop_) = Loop::parse(&text) {
                out.push(loop_);
            }
        }
        out.sort_by_key(|loop_| loop_.id);
        out
    }

    pub fn get(&self, id: u32) -> Option<Loop> {
        self.loops().into_iter().find(|loop_| loop_.id == id)
    }

    pub fn put(&self, loop_: &Loop) -> io::Result<()> {
        fs::write(
            self.dir().join(format!("{:04}.loop", loop_.id)),
            loop_.render(),
        )
    }

    pub fn next_id(&self) -> u32 {
        self.loops().iter().map(|loop_| loop_.id).max().unwrap_or(0) + 1
    }

    /// Открытые петли, где ход у `who`, самые срочные сверху. Это и есть «круг».
    pub fn circle(&self, who: &str, _at: u64) -> Vec<Loop> {
        let mut out: Vec<Loop> = self
            .loops()
            .into_iter()
            .filter(|loop_| loop_.holder() == Some(who) && !loop_.state.closed())
            .collect();
        out.sort_by_key(|loop_| loop_.deadline);
        out
    }

    /// Обещания, которые держит делающий: ими ограничен его WIP.
    pub fn promises_held(&self, who: &str) -> usize {
        self.loops()
            .into_iter()
            .filter(|loop_| loop_.performer == who && loop_.state == State::Promised)
            .count()
    }

    /// Оборот часов по всем петлям. Возвращает строки эскалаций.
    pub fn tick_all(&self, at: u64) -> Vec<String> {
        let mut events = Vec::new();
        for mut loop_ in self.loops() {
            if let Some(event) = loop_.tick(at) {
                let _ = self.put(&loop_);
                events.push(event);
            }
        }
        events
    }

    /// Перепроверить доказательства принятых и объявленных петель.
    ///
    /// Если команда больше не воспроизводит вывод — приёмка отменяется, петля
    /// открывается снова с причиной. Это и есть «след, который может открыться».
    pub fn verify(&self, at: u64) -> Vec<(u32, String)> {
        let mut broken = Vec::new();
        for mut loop_ in self.loops() {
            let Some(evidence) = loop_.evidence.clone() else {
                continue;
            };
            if loop_.state != State::Accepted && loop_.state != State::Declared {
                continue;
            }
            let (code, digest, excerpt) = match run_evidence(&evidence.cmd, &evidence.dir) {
                Ok(result) => result,
                Err(err) => (127, String::new(), err),
            };
            if code != evidence.expect {
                let reason = format!(
                    "доказательство не воспроизвелось: код {code} вместо {} ({})",
                    evidence.expect,
                    excerpt.lines().next().unwrap_or("")
                );
                let _ = loop_.reopen(&reason, at);
                let _ = self.put(&loop_);
                broken.push((loop_.id, reason));
            } else if digest != evidence.digest && !digest.is_empty() {
                let reason = format!("вывод изменился: {}", excerpt.lines().next().unwrap_or(""));
                let _ = loop_.reopen(&reason, at);
                let _ = self.put(&loop_);
                broken.push((loop_.id, reason));
            }
        }
        broken
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ledger(name: &str) -> (Ledger, PathBuf) {
        let dir = std::env::temp_dir().join(format!("chereda-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let ledger = Ledger::open(&dir).unwrap();
        (ledger, dir)
    }

    fn asked(id: u32) -> Loop {
        Loop::new(id, "починить сканер", "ты", "forgessa", Class::Normal, 1000)
    }

    #[test]
    fn move_is_derived_from_the_act_not_from_a_status() {
        let mut loop_ = asked(1);
        assert_eq!(loop_.whose_move(), Some(Side::Performer));
        loop_.promise("forgessa", None, 1001).unwrap();
        assert_eq!(loop_.whose_move(), Some(Side::Performer));
        let evidence = Evidence {
            cmd: "true".to_string(),
            dir: String::new(),
            expect: 0,
            digest: digest("ok"),
            at: 1002,
            excerpt: String::new(),
        };
        loop_.declare("forgessa", evidence, 1002).unwrap();
        assert_eq!(
            loop_.whose_move(),
            Some(Side::Customer),
            "объявил — ход заказчика"
        );
        loop_.accept("ты", 1003).unwrap();
        assert_eq!(loop_.whose_move(), None, "принято — хода нет");
    }

    #[test]
    fn nobody_speaks_out_of_turn() {
        let mut loop_ = asked(1);
        let err = loop_.accept("ты", 1001).unwrap_err();
        assert!(
            err.contains("не в свой ход") || err.contains("ход"),
            "{err}"
        );
        // делающий не может принять сам себе
        let err = loop_.promise("ты", None, 1001).unwrap_err();
        assert!(err.contains("принадлежит"), "{err}");
    }

    #[test]
    fn refusal_and_objection_need_a_reason() {
        let mut loop_ = asked(1);
        assert!(loop_.refuse("forgessa", "", 1001).is_err());
        loop_
            .refuse("forgessa", "нет доступа к хосту", 1001)
            .unwrap();
        assert_eq!(loop_.whose_move(), Some(Side::Customer));
        assert_eq!(loop_.state, State::Refused);
    }

    #[test]
    fn declaration_without_evidence_is_not_a_declaration() {
        let mut loop_ = asked(1);
        loop_.promise("forgessa", None, 1001).unwrap();
        let empty = Evidence {
            cmd: "true".to_string(),
            dir: String::new(),
            expect: 0,
            digest: String::new(),
            at: 1002,
            excerpt: String::new(),
        };
        assert!(loop_.declare("forgessa", empty, 1002).is_err());
    }

    #[test]
    fn clock_expiry_escalates_and_restarts() {
        let mut loop_ = asked(1);
        let expiry = loop_.deadline;
        assert!(loop_.tick(expiry - 1).is_none());
        let event = loop_.tick(expiry).expect("часы истекли");
        assert!(event.contains("уровень выше"), "{event}");
        assert_eq!(loop_.level, 2);
        assert!(loop_.deadline > expiry, "часы пошли заново");
        // третий уровень — край, дальше только повтор
        loop_.tick(loop_.deadline);
        assert_eq!(loop_.level, 3);
        loop_.tick(loop_.deadline);
        assert_eq!(loop_.level, 3, "выше третьего сигнал не идёт");
    }

    #[test]
    fn promise_cannot_exceed_the_class_limit() {
        let mut loop_ = Loop::new(1, "острое дело", "ты", "forgessa", Class::Urgent, 1000);
        loop_.promise("forgessa", Some(10 * 3600), 1001).unwrap();
        assert!(
            loop_.deadline <= 1001 + 4 * 3600,
            "острое дело не обещается на десять часов"
        );
    }

    #[test]
    fn wip_is_counted_in_promises() {
        let (ledger, dir) = ledger("wip");
        for id in 1..=3 {
            let mut loop_ = Loop::new(id, "дело", "ты", "forgessa", Class::Normal, 1000);
            loop_.promise("forgessa", None, 1001).unwrap();
            ledger.put(&loop_).unwrap();
        }
        assert_eq!(ledger.promises_held("forgessa"), 3);
        assert_eq!(ledger.promises_held("другой"), 0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn circle_shows_only_my_moves_most_urgent_first() {
        let (ledger, _dir) = ledger("circle");
        let mut first = Loop::new(1, "срочное", "ты", "forgessa", Class::Normal, 1000);
        first.promise("forgessa", Some(600), 1001).unwrap();
        let mut second = Loop::new(2, "позже", "ты", "forgessa", Class::Normal, 1000);
        second.promise("forgessa", Some(7200), 1001).unwrap();
        let third = Loop::new(3, "чужое", "ты", "другой", Class::Normal, 1000);
        ledger.put(&first).unwrap();
        ledger.put(&second).unwrap();
        ledger.put(&third).unwrap();
        let mine = ledger.circle("forgessa", 1100);
        assert_eq!(mine.len(), 2);
        assert_eq!(mine[0].cause, "срочное", "сначала то, у чего часы короче");
        assert!(
            ledger.circle("ты", 1100).is_empty(),
            "ход заказчика — это не объявлено"
        );
    }

    #[test]
    fn reality_can_revoke_acceptance() {
        let (ledger, dir) = ledger("revoke");
        // доказательство: файл, который потом исчезнет
        let marker = dir.join("ok.txt");
        fs::write(&marker, "ok").unwrap();
        let cmd = format!("test -f {}", marker.display());
        let (code, dg, excerpt) = run_evidence(&cmd, "").unwrap();
        let mut loop_ = asked(1);
        loop_.promise("forgessa", None, 1001).unwrap();
        loop_
            .declare(
                "forgessa",
                Evidence {
                    cmd,
                    dir: String::new(),
                    expect: code,
                    digest: dg,
                    at: 1002,
                    excerpt,
                },
                1002,
            )
            .unwrap();
        loop_.accept("ты", 1003).unwrap();
        ledger.put(&loop_).unwrap();
        assert!(
            ledger.verify(1004).is_empty(),
            "пока доказательство держится"
        );
        fs::remove_file(&marker).unwrap();
        let broken = ledger.verify(1005);
        assert_eq!(broken.len(), 1, "мир отменил приёмку");
        let reopened = ledger.get(1).unwrap();
        assert_eq!(reopened.state, State::Objection);
        assert_eq!(reopened.whose_move(), Some(Side::Performer));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn loop_survives_roundtrip_with_history_and_evidence() {
        let mut loop_ = asked(1);
        loop_.promise("forgessa", Some(900), 1001).unwrap();
        loop_
            .declare(
                "forgessa",
                Evidence {
                    cmd: "cargo test".to_string(),
                    dir: "/tmp".to_string(),
                    expect: 0,
                    digest: "abcd".to_string(),
                    at: 1002,
                    excerpt: "ok".to_string(),
                },
                1002,
            )
            .unwrap();
        let back = Loop::parse(&loop_.render()).unwrap();
        assert_eq!(back.id, 1);
        assert_eq!(back.state, State::Declared);
        assert_eq!(back.history.len(), 3);
        assert_eq!(back.evidence.as_ref().unwrap().cmd, "cargo test");
        assert_eq!(back.whose_move(), Some(Side::Customer));
    }

    #[test]
    fn timing_numbers_do_not_change_the_digest() {
        let a = digest(&normalize("Ran 80 tests in 0.012s\nOK"));
        let b = digest(&normalize("Ran 80 tests in 1.37s\nOK"));
        assert_eq!(a, b);
        let c = digest(&normalize("Ran 82 tests in 1.37s\nOK"));
        assert_ne!(a, c);
    }
}
