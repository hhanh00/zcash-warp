use std::{
    fs::{self, File},
    io::{Cursor, Read as _, Write as _},
    path::PathBuf,
    str::FromStr as _,
    time,
};

use age::{secrecy::ExposeSecret as _, Decryptor};
use anyhow::Result;
use zip::write::FileOptions;

use crate::data::fb::{AGEKeysT, ZipDbConfig, ZipDbConfigT};
use crate::fb_unwrap;
use rusqlite::{backup::Backup, Connection};

pub fn encrypt_zip_database_files(zip_db_config: &ZipDbConfigT) -> Result<()> {
    let ZipDbConfigT {
        directory,
        file_list,
        target_path,
        public_key,
    } = zip_db_config.clone();

    let directory = PathBuf::from(fb_unwrap!(directory));
    let mut zip_directory = directory.clone();
    zip_directory.push(".tmp");
    let _ = fs::create_dir(zip_directory.clone());

    let zip_data = vec![];
    let buff = Cursor::new(zip_data);
    let mut zip_writer = zip::ZipWriter::new(buff);

    let files = fb_unwrap!(file_list);
    for db_name in files.iter() {
        {
            let p = directory.join(db_name);
            tracing::info!("Backup {:?}...", p);
            let src = Connection::open(p)?;
            let mut dst = Connection::open(zip_directory.join(&db_name))?;
            let backup = Backup::new(&src, &mut dst)?;
            backup.run_to_completion(100, time::Duration::from_millis(10), None)?;
        }
        tracing::info!("Zipping {db_name}...");
        {
            zip_writer.start_file(&db_name, FileOptions::<()>::default())?;
            let mut f = File::open(zip_directory.join(db_name))?;
            let mut buffer = Vec::new();
            f.read_to_end(&mut buffer)?;
            zip_writer.write_all(&*buffer)?;
        }
    }
    let buffer = zip_writer.finish()?;
    let zip_data = buffer.into_inner();

    let target_path = fb_unwrap!(target_path);
    let public_key = fb_unwrap!(public_key);
    tracing::info!("Encrypting {target_path}...");
    let public_key = age::x25519::Recipient::from_str(public_key).map_err(anyhow::Error::msg)?;

    let mut encrypted_file = File::create(target_path)?;
    {
        let encryptor = age::Encryptor::with_recipients(vec![Box::new(public_key)]).unwrap();
        let mut writer = encryptor.wrap_output(&mut encrypted_file)?;
        writer.write_all(&*zip_data)?;
        writer.finish()?;
    }
    Ok(())
}

pub fn decrypt_zip_database_files(
    file_path: &str,
    target_directory: &str,
    secret_key: &str,
) -> Result<()> {
    let key = age::x25519::Identity::from_str(secret_key).map_err(anyhow::Error::msg)?;
    let mut encrypted_data = Vec::new();
    {
        let mut f = File::open(file_path)?;
        f.read_to_end(&mut encrypted_data)?;
    }
    let mut zip_data = vec![];
    {
        let Decryptor::Recipients(decryptor) =
            Decryptor::new(&*encrypted_data).map_err(anyhow::Error::msg)?
        else {
            unreachable!()
        };

        let key = &key as &dyn age::Identity;
        let mut reader = decryptor
            .decrypt(std::iter::once(key))
            .map_err(anyhow::Error::msg)?;
        reader.read_to_end(&mut zip_data)?;
    }

    let target_directory = PathBuf::from(target_directory);
    let zip_data = Cursor::new(&*zip_data);
    let mut zip_reader = zip::ZipArchive::new(zip_data)?;
    let file_names: Vec<_> = zip_reader.file_names().map(|s| s.to_string()).collect();
    for file_name in file_names {
        let mut zip_file = zip_reader.by_name(&file_name)?;
        let out_path = target_directory.join(file_name);
        tracing::info!("Unpack to {}", out_path.display());
        let mut out_file = File::create(&out_path)?;
        std::io::copy(&mut zip_file, &mut out_file)?;
    }
    Ok(())
}

pub fn generate_zip_database_keys() -> Result<AGEKeysT> {
    let key = age::x25519::Identity::generate();
    let secret_key = key.to_string().expose_secret().clone();
    let public_key = key.to_public().to_string();
    let keys = AGEKeysT {
        public_key: Some(public_key),
        secret_key: Some(secret_key),
    };
    Ok(keys)
}
