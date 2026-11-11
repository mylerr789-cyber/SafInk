//! Alt süreç başlatma yardımcıları: uygulama ölünce çocuk süreçlerin de ölmesi.
//! Windows'ta sd-server bir job nesnesine bağlanır (sdserver.rs); Linux'ta `PR_SET_PDEATHSIG`.
use std::io;
use std::process::{Child, Command};

/// Çocuğu başlatır; Linux'ta ana süreç ölürse çocuğa SIGKILL gider.
/// `PDEATHSIG`, çocuğu *başlatan iş parçacığı* ölünce tetiklenir; tokio/spawn_blocking iş parçacıkları
/// boşta kalınca sonlandığı için tüm başlatmalar ömrü süreçle aynı olan tek bir iş parçacığında yapılır.
#[cfg(target_os = "linux")]
pub fn spawn_tied(cmd: Command) -> io::Result<Child> {
    use std::os::unix::process::CommandExt;
    use std::sync::mpsc::{channel, Sender};
    type Job = (Command, Sender<io::Result<Child>>);
    static TX: std::sync::OnceLock<parking_lot::Mutex<Sender<Job>>> = std::sync::OnceLock::new();
    let tx = TX.get_or_init(|| {
        let (tx, rx) = channel::<Job>();
        std::thread::Builder::new()
            .name("safink-spawner".into())
            .spawn(move || {
                for (mut cmd, back) in rx {
                    unsafe {
                        cmd.pre_exec(|| {
                            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL as libc::c_ulong, 0, 0, 0);
                            Ok(())
                        });
                    }
                    let _ = back.send(cmd.spawn());
                }
            })
            .expect("spawner thread");
        parking_lot::Mutex::new(tx)
    });
    let (back, rx) = channel();
    tx.lock().send((cmd, back)).map_err(|_| io::Error::other("spawner thread gone"))?;
    rx.recv().map_err(|_| io::Error::other("spawner thread gone"))?
}

#[cfg(not(target_os = "linux"))]
pub fn spawn_tied(mut cmd: Command) -> io::Result<Child> { cmd.spawn() }
