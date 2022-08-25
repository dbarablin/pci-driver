// SPDX-License-Identifier: MIT OR Apache-2.0

/* ---------------------------------------------------------------------------------------------- */

use std::collections::{BTreeSet, HashMap};
use std::fmt::Debug;
use std::fs::{File, OpenOptions};
use std::io::{self, ErrorKind};
use std::mem;
use std::os::unix::io::AsRawFd;

use crate::backends::vfio::bindings::{
    vfio_group_status, VFIO_TYPE1v2_IOMMU, VFIO_API_VERSION, VFIO_GROUP_FLAGS_VIABLE,
};
use crate::backends::vfio::ioctl::{
    vfio_check_extension, vfio_get_api_version, vfio_group_get_status, vfio_group_set_container,
    vfio_set_iommu,
};

/* ---------------------------------------------------------------------------------------------- */

fn open_group(group_number: u32) -> io::Result<File> {
    // open group

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(format!("/dev/vfio/{}", group_number))?;

    // check if group is viable

    let mut group_status = vfio_group_status {
        argsz: mem::size_of::<vfio_group_status>() as u32,
        flags: 0,
    };

    unsafe { vfio_group_get_status(file.as_raw_fd(), &mut group_status)? };

    if group_status.flags & VFIO_GROUP_FLAGS_VIABLE == 0 {
        return Err(io::Error::new(
            ErrorKind::Other,
            "Group is not viable; are all devices in the group bound to vfio or unbound?",
        ));
    }

    // success

    Ok(file)
}

/* ---------------------------------------------------------------------------------------------- */

/// A VFIO container representing an IOMMU context that may contain zero or more VFIO groups.
#[derive(Debug)]
pub struct VfioContainer {
    group_numbers: Box<[u32]>,
    pub(crate) groups: HashMap<u32, File>,
}

impl VfioContainer {
    /// Creates a new, empty [`VfioContainer`].
    ///
    /// This fails if not all devices in all given groups have been bound to vfio-pci (the VFIO docs
    /// say "it's also sufficient to only unbind the device from host drivers if a VFIO driver is
    /// unavailable").
    ///
    /// This fails if any of the groups is already open elsewhere, for instance if another
    /// [`VfioContainer`] containing one of the groups already currently exists.
    pub fn new(groups: &[u32]) -> io::Result<VfioContainer> {
        // open groups

        let group_numbers = Vec::from(groups)
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Box<[_]>>();

        let groups: HashMap<_, _> = group_numbers
            .iter()
            .map(|&n| Ok((n, open_group(n)?)))
            .collect::<io::Result<_>>()?;

        // create container

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/vfio/vfio")?;

        let fd = file.as_raw_fd();

        // check API version

        if unsafe { vfio_get_api_version(fd)? } != VFIO_API_VERSION as i32 {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "Wrong VFIO_API_VERSION",
            ));
        }

        // check extension

        if unsafe { vfio_check_extension(fd, VFIO_TYPE1v2_IOMMU as i32)? } != 1 {
            return Err(io::Error::new(ErrorKind::InvalidInput, "TODO"));
        }

        // add groups to container

        for group_file in groups.values() {
            unsafe { vfio_group_set_container(group_file.as_raw_fd(), &fd)? };
        }

        // enable IOMMU

        unsafe { vfio_set_iommu(fd, VFIO_TYPE1v2_IOMMU as i32)? };

        // success

        Ok(VfioContainer {
            group_numbers,
            groups,
        })
    }

    /// The group numbers of the groups this container contains.
    ///
    /// In ascending order, without duplicates.
    pub fn groups(&self) -> &[u32] {
        &self.group_numbers
    }
}

/* ---------------------------------------------------------------------------------------------- */
