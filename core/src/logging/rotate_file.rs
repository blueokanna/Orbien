use chrono::{Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const BACKUP_TIME_FORMAT: &str = "%Y%m%d-%H%M%S";
const BACKUP_TIME_LEN: usize = 15;

pub struct RotateFileWriter {
    path: PathBuf,
    dir: PathBuf,
    ext: String,
    backup_prefix: String,
    max_days: u32,
    file: Option<File>,
    day: Option<NaiveDate>,
}

impl RotateFileWriter {
    pub fn open(path: impl Into<PathBuf>, max_days: u32) -> io::Result<Self> {
        let path = path.into();
        let dir = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        fs::create_dir_all(&dir)?;

        let filename = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "orbien.log".into());
        let ext = path
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        let stem = filename
            .strip_suffix(&ext)
            .unwrap_or(filename.as_str())
            .to_string();

        let mut writer = Self {
            path,
            dir,
            ext,
            backup_prefix: format!("{stem}."),
            max_days,
            file: None,
            day: None,
        };
        writer.open_existing_or_new()?;
        writer.day = Some(Local::now().date_naive());
        Ok(writer)
    }

    #[inline]
    fn ensure_ready(&mut self) -> io::Result<()> {
        let today = Local::now().date_naive();
        if self.day == Some(today) && self.file.is_some() {
            return Ok(());
        }
        if self.day.is_some() && self.day != Some(today) {
            return self.rotate();
        }
        self.open_existing_or_new()?;
        self.day = Some(today);
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        if let Some(mut file) = self.file.take() {
            let _ = file.flush();
        }
        self.open_new()?;
        let _ = self.clear_old_backups();
        Ok(())
    }

    fn open_existing_or_new(&mut self) -> io::Result<()> {
        match OpenOptions::new().append(true).open(&self.path) {
            Ok(file) => {
                self.file = Some(file);
                Ok(())
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => self.open_new(),
            Err(e) => Err(e),
        }
    }

    fn open_new(&mut self) -> io::Result<()> {
        let now = Local::now();
        if self.path.exists() {
            let backup = self.dir.join(format!(
                "{}{}{}",
                self.backup_prefix,
                now.format(BACKUP_TIME_FORMAT),
                self.ext
            ));
            fs::rename(&self.path, &backup)?;
        }
        self.file = Some(open_new_file(&self.path)?);
        self.day = Some(now.date_naive());
        Ok(())
    }

    fn clear_old_backups(&self) -> io::Result<()> {
        if self.max_days == 0 {
            return Ok(());
        }
        let cutoff = Local::now() - chrono::Duration::days(self.max_days as i64);
        let prefix = self.backup_prefix.as_str();
        let ext = self.ext.as_str();

        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let Some(ts) = parse_backup_timestamp(prefix, ext, &name) else {
                continue;
            };
            let Some(dt) = Local.from_local_datetime(&ts).single() else {
                continue;
            };
            if dt < cutoff {
                let _ = fs::remove_file(entry.path());
            }
        }
        Ok(())
    }
}

impl Write for RotateFileWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.ensure_ready()?;
        self.file.as_mut().unwrap().write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(file) = self.file.as_mut() {
            file.flush()?;
        }
        Ok(())
    }
}

impl Drop for RotateFileWriter {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

fn open_new_file(path: &Path) -> io::Result<File> {
    let mut opts = OpenOptions::new();
    opts.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)
}

fn parse_backup_timestamp(prefix: &str, ext: &str, name: &str) -> Option<NaiveDateTime> {
    if !name.starts_with(prefix) || !name.ends_with(ext) {
        return None;
    }
    if name.len() <= prefix.len() + ext.len() {
        return None;
    }
    let mid = &name[prefix.len()..name.len() - ext.len()];
    if mid.len() != BACKUP_TIME_LEN || mid.as_bytes().get(8) != Some(&b'-') {
        return None;
    }
    let date = NaiveDate::parse_from_str(&mid[..8], "%Y%m%d").ok()?;
    let time = NaiveTime::parse_from_str(&mid[9..], "%H%M%S").ok()?;
    Some(date.and_time(time))
}
