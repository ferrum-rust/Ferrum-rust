use anyhow::{ensure, Context, Result};
use sha1::{Digest, Sha1};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
const LIMIT: u64 = 50 * 1024 * 1024;
pub fn download(url: &str, expected: &str, dir: &Path) -> Result<PathBuf> {
    let u: ureq::http::Uri = url.parse()?;
    ensure!(
        matches!(u.scheme_str(), Some("http" | "https"))
            && u.host().is_some()
            && u.authority().is_some_and(|a| !a.as_str().contains('@')),
        "Unsupported resource pack address"
    );
    fs::create_dir_all(dir)?;
    let expected = expected.to_ascii_lowercase();
    let verify = expected.len() == 40 && expected.bytes().all(|b| b.is_ascii_hexdigit());
    if verify {
        let p = dir.join(format!("{expected}.zip"));
        if let Ok(f) = fs::File::open(&p) {
            let mut sha = Sha1::new();
            std::io::copy(&mut f.take(LIMIT + 1), &mut sha)?;
            if format!("{:x}", sha.finalize()) == expected {
                return Ok(p);
            }
        }
    }
    let response = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(60)))
        .build()
        .new_agent()
        .get(url)
        .call()?;
    let mut data = Vec::new();
    response
        .into_body()
        .into_reader()
        .take(LIMIT + 1)
        .read_to_end(&mut data)?;
    ensure!(data.len() as u64 <= LIMIT, "Resource pack exceeds 50 MiB");
    let digest = format!("{:x}", Sha1::digest(&data));
    ensure!(
        !verify || digest == expected,
        "Server resource pack SHA-1 does not match"
    );
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&data))?;
    ensure!(
        zip.by_name("pack.mcmeta")
            .context("Missing pack.mcmeta")?
            .size()
            <= 1_048_576,
        "Resource pack metadata too large"
    );
    let dest = dir.join(format!("{digest}.zip"));
    let tmp = dir.join(format!("{digest}-{:016x}.part", rand::random::<u64>()));
    let mut f = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&tmp)?;
    let result = (|| -> Result<()> {
        f.write_all(&data)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, &dest)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
        result?;
    }
    Ok(dest)
}

#[cfg(test)]
mod tests{
    use super::*;
    use std::net::TcpListener;
    #[test]
    fn only_a_valid_matching_server_archive_reaches_the_cache(){
        let mut z=zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        z.start_file("pack.mcmeta",zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(br#"{"pack":{"pack_format":1,"description":"test"}}"#).unwrap();
        let data=z.finish().unwrap().into_inner();let expected=format!("{:x}",Sha1::digest(&data));
        let dir=std::env::temp_dir().join(format!("ferrum-pack-{:016x}",rand::random::<u64>()));
        let l=TcpListener::bind("127.0.0.1:0").unwrap();let address=format!("http://{}/pack.zip",l.local_addr().unwrap());
        let server=std::thread::spawn(move||{for stream in l.incoming().take(2){let mut c=stream.unwrap();let mut buf=[0;2048];let _=c.read(&mut buf).unwrap();
            write!(c,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",data.len()).unwrap();c.write_all(&data).unwrap();}});
        assert!(download("file:///unrelated.zip",&expected,&dir).is_err());
        assert!(download(&address,&"0".repeat(40),&dir).is_err());
        let p=download(&address,&expected,&dir).unwrap();server.join().unwrap();
        assert_eq!(download(&address,&expected,&dir).unwrap(),p);
        assert!(crate::resource_packs::Stack::with_server(&[],&p).is_ok());
        fs::remove_file(p).unwrap();fs::remove_dir(dir).unwrap();
    }
}
