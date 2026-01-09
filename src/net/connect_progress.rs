use tokio::sync::mpsc::UnboundedSender;

#[derive(Debug, Clone)]
pub enum ConnectProgress {
    Stage(String),
    Log(String),
    GameLaunched { exe_path: String },
    Download {
        label: String,
        done_bytes: u64,
        total_bytes: Option<u64>,
    },
}

pub type ProgressTx = UnboundedSender<ConnectProgress>;

pub fn stage(tx: Option<&ProgressTx>, message: impl Into<String>) {
    let Some(tx) = tx else {
        return;
    };
    let _ = tx.send(ConnectProgress::Stage(message.into()));
}

pub fn log(tx: Option<&ProgressTx>, line: impl Into<String>) {
    let Some(tx) = tx else {
        return;
    };
    let _ = tx.send(ConnectProgress::Log(line.into()));
}

pub fn game_launched(tx: Option<&ProgressTx>, exe_path: impl Into<String>) {
    let Some(tx) = tx else {
        return;
    };
    let _ = tx.send(ConnectProgress::GameLaunched {
        exe_path: exe_path.into(),
    });
}

pub fn download(
    tx: Option<&ProgressTx>,
    label: impl Into<String>,
    done_bytes: u64,
    total_bytes: Option<u64>,
) {
    let Some(tx) = tx else {
        return;
    };
    let _ = tx.send(ConnectProgress::Download {
        label: label.into(),
        done_bytes,
        total_bytes,
    });
}
