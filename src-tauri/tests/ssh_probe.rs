//! Диагностика SSH-аутентификации libgit2/libssh2 — не часть обычного прогона,
//! запускается вручную, когда «клонирование зависло на аутентификации».
//!
//! Проверяет три источника учётных данных на живом сервере и печатает, на каком
//! шаге всё встало. Именно этим тестом был пойман бэкенд WinCNG: вариант «ключ
//! из памяти» (тот, которым ходит приложение) висел вечно, потому что libgit2
//! крутит `libssh2_userauth_publickey_frommemory` в цикле по EAGAIN, а WinCNG
//! этот вызов не поддерживает.
//!
//! ```text
//! PROBE_URL=ssh://git@host/group/repo.git PROBE_KEY=C:\Users\me\.ssh\id_ed25519 \
//!   cargo test --test ssh_probe -- --ignored --nocapture
//! ```
//! Оба параметра обязательны: URL корпоративного репозитория в коде не хранится,
//! а ключ берётся тот, которым авторизуется пользователь.

use std::io::Write;
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use git2::{Cred, CredentialType, RemoteCallbacks};

/// Сколько ждём один вариант, прежде чем объявить его зависшим. Зависание —
/// это и есть искомый симптом, поэтому бюджет заметно больше здорового ответа
/// (полторы секунды), но меньше терпения человека.
const BUDGET: Duration = Duration::from_secs(45);

fn log(start: Instant, msg: &str) {
    println!("  [{:>6.2}s] {msg}", start.elapsed().as_secs_f64());
    std::io::stdout().flush().ok();
}

/// Как получить учётные данные в очередном варианте. `String` вместо `&Path`,
/// потому что замыкание уезжает в отдельный поток.
enum Source {
    Memory(String),
    File(String),
    Agent,
}

impl Source {
    fn cred(&self, user: &str) -> Result<Cred, git2::Error> {
        match self {
            Source::Memory(path) => {
                let material = std::fs::read_to_string(path)
                    .map_err(|e| git2::Error::from_str(&e.to_string()))?;
                Cred::ssh_key_from_memory(user, None, &material, None)
            }
            Source::File(path) => Cred::ssh_key(user, None::<&Path>, Path::new(path), None),
            Source::Agent => Cred::ssh_key_from_agent(user),
        }
    }
}

/// Один вариант, доведённый до списка ссылок (аналог `git ls-remote`): на диск
/// ничего не пишется, но проходится ровно тот же путь, что и у клона —
/// рукопожатие, ключ хоста, аутентификация.
fn probe(name: &str, url: String, source: Source) {
    println!("\n=== вариант: {name} ===");
    let (tx, rx) = mpsc::channel();
    // Поток намеренно не join-ится при таймауте: он остаётся висеть внутри
    // блокирующего вызова libssh2 и не отзывается ни на что, кроме выхода
    // процесса — что и делает зависание наблюдаемым.
    std::thread::spawn(move || {
        let start = Instant::now();
        let mut callbacks = RemoteCallbacks::new();
        callbacks.credentials(move |_url, username, allowed| {
            log(start, &format!("запрошены credentials, сервер разрешает {allowed:?}"));
            if !allowed.contains(CredentialType::SSH_KEY) {
                return Err(git2::Error::from_str("сервер не предлагает publickey"));
            }
            let cred = source.cred(username.unwrap_or("git"));
            log(
                start,
                &match &cred {
                    Ok(_) => "ключ отдан libssh2".to_string(),
                    Err(e) => format!("ключ не собран: {}", e.message()),
                },
            );
            cred
        });
        callbacks.certificate_check(move |_cert, host| {
            log(start, &format!("ключ хоста {host} принят"));
            Ok(git2::CertificateCheckStatus::CertificateOk)
        });

        let result = (|| -> Result<usize, git2::Error> {
            let mut remote = git2::Remote::create_detached(url.as_str())?;
            remote.connect_auth(git2::Direction::Fetch, Some(callbacks), None)?;
            Ok(remote.list()?.len())
        })();

        let elapsed = start.elapsed().as_secs_f64();
        let _ = tx.send(match result {
            Ok(n) => format!("OK: получено {n} ссылок за {elapsed:.2}s"),
            Err(e) => format!(
                "ОШИБКА за {elapsed:.2}s: class={:?} code={:?} {}",
                e.class(),
                e.code(),
                e.message()
            ),
        });
    });

    match rx.recv_timeout(BUDGET) {
        Ok(msg) => println!("  -> {msg}"),
        Err(_) => println!("  -> ЗАВИС: ответа нет {}s", BUDGET.as_secs()),
    }
}

#[test]
#[ignore = "диагностика: нужен доступ к серверу, параметры в PROBE_URL/PROBE_KEY"]
fn ssh_auth_sources() {
    let url = std::env::var("PROBE_URL").expect("задайте PROBE_URL=ssh://git@host/group/repo.git");
    let key = std::env::var("PROBE_KEY").expect("задайте PROBE_KEY=путь до приватного ключа");
    println!("URL: {url}\nключ: {key}");

    probe(
        "ключ из памяти (этим путём ходит приложение)",
        url.clone(),
        Source::Memory(key.clone()),
    );
    probe("ключ из файла", url.clone(), Source::File(key));
    probe("ssh-агент", url, Source::Agent);
}
