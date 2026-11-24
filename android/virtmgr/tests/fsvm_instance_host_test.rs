// Copyright (C) 2026 DiamaneOS
// The generated AIDL shape has no Clone implementation. Production helpers are included verbatim.
extern crate self as anyhow;
extern crate self as binder;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub struct ParcelFileDescriptor(std::fs::File);
impl ParcelFileDescriptor {
    pub fn new(file: std::fs::File) -> Self {
        Self(file)
    }
}
mod aidl {
    pub struct Partition {
        pub label: String,
        pub image: Option<super::ParcelFileDescriptor>,
        pub writable: bool,
        pub guid: Option<String>,
    }
}
#[path = "../src/fsvm_client.rs"]
mod fsvm_client;
#[path = "../src/fsvm_instance.rs"]
mod fsvm_instance;

#[macro_export]
macro_rules! ensure {
    ($condition:expr, $message:expr) => {
        if !$condition {
            return Err($message.into());
        }
    };
}

#[test]
fn supplied_instance_bytes_never_reach_the_composite_input() {
    use std::io::{Read, Write};
    use std::os::unix::fs::MetadataExt;
    let directory = std::env::temp_dir().join(format!("fsvm-private-input-{}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    let supplied_path = directory.join("supplied");
    std::fs::File::create(&supplied_path)
        .unwrap()
        .write_all(b"QFI\xfb hostile image header")
        .unwrap();
    let supplied_file = std::fs::File::open(&supplied_path).unwrap();
    let supplied_inode = supplied_file.metadata().unwrap().ino();
    let supplied = [aidl::Partition {
        label: "vm-instance".to_string(),
        image: Some(ParcelFileDescriptor::new(supplied_file)),
        writable: false,
        guid: Some("untrusted".to_string()),
    }];
    let mut partitions = fsvm_instance::partitions(&supplied, &directory).unwrap();
    assert_eq!(partitions.len(), 1);
    assert!(partitions[0].writable);
    assert!(partitions[0].guid.is_none());
    let file = &mut partitions[0].image.as_mut().unwrap().0;
    assert_ne!(file.metadata().unwrap().ino(), supplied_inode);
    assert_eq!(file.metadata().unwrap().len(), 10 << 20);
    let mut header = [0; 21];
    file.read_exact(&mut header).unwrap();
    assert_eq!(&header, b"Android-VM-instance\x01\x00");
    assert_eq!(std::fs::read(&supplied_path).unwrap(), b"QFI\xfb hostile image header");
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn private_instance_shape_is_checked_before_creation() {
    let directory = std::env::temp_dir().join(format!("fsvm-private-shape-{}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    let bad =
        [aidl::Partition { label: "card".to_string(), image: None, writable: false, guid: None }];
    assert!(fsvm_instance::partitions(&bad, &directory).is_err());
    assert!(fsvm_instance::partitions(&[], &directory).is_err());
    assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 0);
    std::fs::remove_dir_all(directory).unwrap();
}
