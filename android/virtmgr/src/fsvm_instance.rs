// Copyright (C) 2026 DiamaneOS
//! The stateless filesystem VM instance is created only by the manager.
use crate::aidl;
use anyhow::{ensure, Result};
use binder::ParcelFileDescriptor;
use std::path::Path;

// DiamaneOS: discard the client's instance FD before any composite image assembly.
pub fn partitions(supplied: &[aidl::Partition], directory: &Path) -> Result<Vec<aidl::Partition>> {
    ensure!(supplied.len() == 1, "expected one private instance partition");
    ensure!(
        supplied[0].label == crate::fsvm_client::INSTANCE_PARTITION,
        "unexpected instance partition"
    );
    Ok(vec![aidl::Partition {
        label: crate::fsvm_client::INSTANCE_PARTITION.to_string(),
        image: Some(ParcelFileDescriptor::new(crate::fsvm_client::instance_file(directory)?)),
        writable: true,
        guid: None,
    }])
}
