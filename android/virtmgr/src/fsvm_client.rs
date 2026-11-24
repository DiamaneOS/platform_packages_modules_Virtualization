// Copyright (C) 2026 DiamaneOS
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! DiamaneOS: the filesystem VM's hosts (vendor/diamaneos fsvm) and what they may ask for.
//!
//! fsvmd serves SD cards and USB drives, fsvmd_obb an app's OBB image. Each starts its own VM manager, whose previous
//! SELinux context is the client's domain. The VM manager names the client once, from that domain, at the start of
//! createVm, and every later check uses that answer: the request's shape here, the disks' labels in
//! virtualmachine.rs, the raw disks and the card's geometry in crosvm.rs. The client's executable is not asked: it
//! may not be readable, and a client that could not be named would lose these checks.
//!
//! A filesystem VM is protected, has a raw config, and gets its volumes as whole images: crosvm opens each one raw.
//! Its one other disk is the instance disk, a partition list of exactly the `vm-instance` partition, which the VM
//! manager made itself (initializeWritablePartition). A volume in a partition list would make the VM manager read
//! its header for a composite disk, a parser the filesystem VM is there to keep away from the host.

use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

/// A fresh instance disk for these stateless guests, created only by the manager.
/// No byte from the client's instance descriptor reaches composite image probing.
pub fn instance_file(directory: &Path) -> std::io::Result<File> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(directory.join("fsvm-instance.img"))?;
    file.set_len(10 << 20)?;
    file.write_all(b"Android-VM-instance\x01\x00")?;
    file.seek(SeekFrom::Start(0))?;
    Ok(file)
}

/// Check the original request before any app payload or config parser is called.
pub fn check_variant(caller: Option<FsvmCaller>, app_config: bool) -> Result<(), String> {
    if let Some(caller) = caller {
        if app_config {
            return Err(format!("{caller:?} filesystem VM: an app config, not a raw config"));
        }
    }
    Ok(())
}

/// Which filesystem VM host calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsvmCaller {
    Card,
    Obb,
}

impl FsvmCaller {
    /// The client named by its SELinux domain, the type of the VM manager's previous context.
    pub fn from_domain(domain: &str) -> Option<FsvmCaller> {
        match domain {
            "fsvmd" => Some(FsvmCaller::Card),
            "fsvmd_obb" => Some(FsvmCaller::Obb),
            _ => None,
        }
    }
}

/// One disk of a request as far as the checks here need it: whether it is a whole image, and the labels of its
/// partitions.
pub struct DiskShape<'a> {
    pub whole_image: bool,
    pub partitions: Vec<&'a str>,
}

/// The label of the one partition a filesystem VM's partition list may hold.
pub const INSTANCE_PARTITION: &str = "vm-instance";

/// Whether a filesystem VM host's request has the only shape it may have: a protected raw config, whole-image
/// volumes and at most one instance disk.
pub fn check_request(
    caller: FsvmCaller,
    app_config: bool,
    protected: bool,
    disks: &[DiskShape],
) -> Result<(), String> {
    if app_config {
        return Err(format!("{caller:?} filesystem VM: an app config, not a raw config"));
    }
    if !protected {
        return Err(format!("{caller:?} filesystem VM: not a protected VM"));
    }
    let mut instance_disks = 0;
    for (index, disk) in disks.iter().enumerate() {
        match (disk.whole_image, disk.partitions.as_slice()) {
            (true, []) => {}
            (false, [INSTANCE_PARTITION]) => instance_disks += 1,
            _ => {
                return Err(format!(
                    "{caller:?} filesystem VM: disk {index} is not a whole image (partitions {:?})",
                    disk.partitions
                ))
            }
        }
    }
    if instance_disks > 1 {
        return Err(format!("{caller:?} filesystem VM: {instance_disks} instance disks"));
    }
    Ok(())
}

/// The disks each filesystem VM host may pass, and nothing else.
pub fn disk_label(caller: FsvmCaller, label: &str, writable: bool) -> Result<(), String> {
    match (caller, label) {
        (FsvmCaller::Card, "vold_device") => Ok(()),
        (FsvmCaller::Obb, "media_rw_data_file" | "fuse") if !writable => Ok(()),
        (FsvmCaller::Obb, "media_rw_data_file" | "fuse") => {
            Err(format!("fsvmd_obb passed a writable OBB image ({label})"))
        }
        _ => Err(format!("{caller:?} filesystem VM may not use a disk labelled {label}")),
    }
}

/// The logical block size crosvm must give a raw disk: None for its default of 512 bytes, Some(4096) for a block
/// device with 4096-byte logical blocks. `is_block` is whether the descriptor is a block device, `sector` asks a
/// block device for its logical block size (BLKSSZGET). A block device whose size cannot be read, or whose size
/// crosvm cannot give the guest as it is, gets no VM: the guest would address the wrong part of the disk.
pub fn block_size(
    is_block: Result<bool, String>,
    sector: impl FnOnce() -> Result<i32, String>,
) -> Result<Option<u32>, String> {
    if !is_block.map_err(|e| format!("the disk's type: {e}"))? {
        return Ok(None);
    }
    match sector().map_err(|e| format!("the block device's logical block size: {e}"))? {
        512 => Ok(None),
        4096 => Ok(Some(4096)),
        other => Err(format!("the block device has {other}-byte logical blocks, not 512 or 4096")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use FsvmCaller::*;

    fn whole() -> DiskShape<'static> {
        DiskShape { whole_image: true, partitions: vec![] }
    }

    fn list(labels: &[&'static str]) -> DiskShape<'static> {
        DiskShape { whole_image: false, partitions: labels.to_vec() }
    }

    #[test]
    fn app_payload_is_not_opened_for_a_filesystem_host() {
        for caller in [Card, Obb] {
            let result = check_variant(Some(caller), true)
                .map(|()| panic!("forbidden app reached payload conversion"));
            assert!(result.is_err());
        }
        assert!(check_variant(None, true).is_ok());
    }

    #[test]
    fn manager_instance_is_fresh_and_cannot_be_substituted() {
        use std::io::Read;
        let directory =
            std::env::temp_dir().join(format!("fsvm-instance-test-{}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let mut instance = instance_file(&directory).unwrap();
        assert_eq!(instance.metadata().unwrap().len(), 10 << 20);
        let mut header = [0; 21];
        instance.read_exact(&mut header).unwrap();
        assert_eq!(&header, b"Android-VM-instance\x01\x00");
        assert!(instance_file(&directory).is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn clients_are_named_by_their_domain_alone() {
        assert_eq!(FsvmCaller::from_domain("fsvmd"), Some(Card));
        assert_eq!(FsvmCaller::from_domain("fsvmd_obb"), Some(Obb));
        for other in
            ["untrusted_app", "shell", "system_server", "fsvmd_virtualizationmanager", "", "fsvmd2"]
        {
            assert_eq!(FsvmCaller::from_domain(other), None, "{other}");
        }
    }

    #[test]
    fn what_fsvmd_sends_passes() {
        for caller in [Card, Obb] {
            assert!(check_request(caller, false, true, &[whole(), list(&["vm-instance"])]).is_ok());
            assert!(check_request(
                caller,
                false,
                true,
                &[whole(), whole(), whole(), list(&["vm-instance"])]
            )
            .is_ok());
        }
    }

    #[test]
    fn a_volume_in_a_partition_list_is_refused() {
        for caller in [Card, Obb] {
            assert!(check_request(caller, false, true, &[list(&["card"]), list(&["vm-instance"])])
                .is_err());
            assert!(check_request(caller, false, true, &[list(&["vm-instance", "card"])]).is_err());
            assert!(check_request(caller, false, true, &[list(&[])]).is_err());
            assert!(check_request(
                caller,
                false,
                true,
                &[list(&["vm-instance"]), list(&["vm-instance"])]
            )
            .is_err());
        }
    }

    #[test]
    fn only_protected_raw_vms() {
        for caller in [Card, Obb] {
            assert!(check_request(caller, false, false, &[whole()]).is_err());
            assert!(check_request(caller, true, true, &[whole()]).is_err());
        }
    }

    #[test]
    fn filesystem_vm_disk_labels() {
        // Cards: their block device, read and written. Nothing else.
        assert!(disk_label(Card, "vold_device", true).is_ok());
        assert!(disk_label(Card, "vold_device", false).is_ok());
        for other in
            ["media_rw_data_file", "fuse", "app_data_file", "system_file", "shell_data_file", "x"]
        {
            assert!(disk_label(Card, other, false).is_err(), "card {other}");
        }
        // OBB images: from shared storage or a card, only read. Never a card itself.
        for obb in ["media_rw_data_file", "fuse"] {
            assert!(disk_label(Obb, obb, false).is_ok(), "{obb}");
            assert!(disk_label(Obb, obb, true).is_err(), "{obb} writable");
        }
        for other in
            ["vold_device", "app_data_file", "system_file", "virtualizationservice_data_file", "x"]
        {
            assert!(disk_label(Obb, other, false).is_err(), "obb {other}");
        }
    }

    #[test]
    fn a_card_needs_a_known_geometry() {
        assert_eq!(block_size(Ok(true), || Ok(512)), Ok(None));
        assert_eq!(block_size(Ok(true), || Ok(4096)), Ok(Some(4096)));
        assert!(block_size(Ok(true), || Err("denied".into())).is_err());
        assert!(block_size(Ok(true), || Ok(2048)).is_err());
        assert!(block_size(Ok(true), || Ok(0)).is_err());
        assert!(block_size(Err("fstat".into()), || Ok(4096)).is_err());
    }

    #[test]
    fn a_file_keeps_the_default() {
        assert_eq!(block_size(Ok(false), || panic!("a file is not asked")), Ok(None));
    }
}
