//! Avisos no celular pelo ntfy (app ntfy no Android/iOS, servidor ntfy.sh ou o seu): GPU quente,
//! disco falhando, servidor de IA caiu, PC saiu da rede, undervolt terminou. A configuração fica
//! em /var/lib/numapilot/notify.json (o serviço grava; todos leem para mandar avisos).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Notify {
    pub enabled: bool,
    /// https://ntfy.sh ou o seu servidor
    pub server: String,
    /// Tópico (o "canal" que o app do celular assina). Use um nome difícil de adivinhar.
    pub topic: String,
    /// Token de acesso (servidor com senha), opcional.
    pub token: Option<String>,
    pub gpu_hot: bool,
    pub gpu_hot_c: f64,
    pub health: bool,
    pub llm: bool,
    pub peers: bool,
    pub jobs: bool,
}

impl Default for Notify {
    fn default() -> Self {
        Notify {
            enabled: false,
            server: "https://ntfy.sh".into(),
            topic: String::new(),
            token: None,
            gpu_hot: true,
            gpu_hot_c: 90.0,
            health: true,
            llm: true,
            peers: true,
            jobs: true,
        }
    }
}

impl Notify {
    pub fn path() -> std::path::PathBuf {
        crate::learned::state_dir().join("notify.json")
    }

    pub fn load() -> Notify {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(crate::learned::state_dir())?;
        std::fs::write(
            Self::path(),
            serde_json::to_string_pretty(self).unwrap_or_default(),
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        if !(self.server.starts_with("https://") || self.server.starts_with("http://")) {
            return Err(crate::i18n::tr("server must start with https://").into());
        }
        if self.topic.len() < 6
            || !self
                .topic
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(crate::i18n::tr("topic: at least 6 letters, numbers, - or _").into());
        }
        Ok(())
    }

    /// Link para assinar no app (também abre no navegador).
    pub fn subscribe_url(&self) -> String {
        format!("{}/{}", self.server.trim_end_matches('/'), self.topic)
    }

    /// Manda já (sem cooldown). prio: 1 mínima .. 5 urgente.
    pub fn send_now(&self, title: &str, body: &str, prio: u8, tags: &[&str]) -> Result<(), String> {
        if self.topic.is_empty() {
            return Err(crate::i18n::tr("no topic configured").into());
        }
        let host = std::fs::read_to_string("/etc/hostname")
            .unwrap_or_default()
            .trim()
            .to_string();
        let msg = serde_json::json!({
            "topic": self.topic,
            "title": if host.is_empty() { title.to_string() } else { format!("[{host}] {title}") },
            "message": body,
            "priority": prio.clamp(1, 5),
            "tags": tags,
        });
        let mut c = std::process::Command::new("curl");
        c.args([
            "-fsS",
            "--max-time",
            "10",
            "-H",
            "Content-Type: application/json",
        ]);
        if let Some(t) = self.token.as_deref().filter(|t| !t.is_empty()) {
            c.arg("-H").arg(format!("Authorization: Bearer {t}"));
        }
        let o = c
            .arg("-d")
            .arg(msg.to_string())
            .arg(self.server.trim_end_matches('/'))
            .output()
            .map_err(|e| format!("curl: {e}"))?;
        if o.status.success() {
            Ok(())
        } else {
            Err(format!(
                "ntfy: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            ))
        }
    }
}

static LAST: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);

/// Manda um aviso se esse tipo estiver ligado e o mesmo `key` não foi avisado nos últimos
/// `cooldown`. Não trava: roda em segundo plano.
pub fn event(kind: &str, key: &str, title: &str, body: &str, prio: u8, cooldown: Duration) {
    let n = Notify::load();
    let on = n.enabled
        && match kind {
            "gpu_hot" => n.gpu_hot,
            "health" => n.health,
            "llm" => n.llm,
            "peers" => n.peers,
            _ => n.jobs,
        };
    if !on {
        return;
    }
    {
        let mut g = LAST.lock().unwrap_or_else(|e| e.into_inner());
        let m = g.get_or_insert_with(HashMap::new);
        if m.get(key).is_some_and(|t| t.elapsed() < cooldown) {
            return;
        }
        m.insert(key.to_string(), Instant::now());
    }
    let (title, body) = (title.to_string(), body.to_string());
    let tags: &'static [&'static str] = match prio {
        5 | 4 => &["warning"],
        _ => &["information_source"],
    };
    std::thread::spawn(move || {
        if let Err(e) = n.send_now(&title, &body, prio, tags) {
            crate::nlog!("notification not sent: {e}");
        }
    });
}

/// Tópico novo e difícil de adivinhar.
pub fn random_topic() -> String {
    let mut b = [0u8; 8];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = f.read_exact(&mut b);
    }
    format!(
        "numapilot-{}",
        b.iter().map(|x| format!("{x:02x}")).collect::<String>()
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn validate() {
        let mut n = super::Notify {
            enabled: true,
            topic: "numapilot-abc123".into(),
            ..Default::default()
        };
        assert!(n.validate().is_ok());
        assert_eq!(n.subscribe_url(), "https://ntfy.sh/numapilot-abc123");
        n.topic = "a b".into();
        assert!(n.validate().is_err());
        assert!(super::random_topic().starts_with("numapilot-"));
        assert_eq!(super::random_topic().len(), "numapilot-".len() + 16);
    }
}
