// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2022 Oxide Computer Company

use crate::error::Error;
use crate::illumos;
use std::{ffi, os, ptr};

//use libdladm_sys as dladm;

pub(crate) fn get_handle() -> Result<illumos::dladm_handle_t, Error> {
    let mut handle: illumos::dladm_handle_t = ptr::null_mut();
    let status = unsafe { illumos::dladm_open(&mut handle) };
    if status != illumos::dladm_status_t_DLADM_STATUS_OK {
        return Err(Error::Dladm(String::from("get handle"), status));
    }

    Ok(handle)
}

pub(crate) fn link_id(
    name: &String,
    h: *mut illumos::dladm_handle,
) -> Result<illumos::datalink_id_t, Error> {
    let mut id: illumos::datalink_id_t = 0;
    let linkname = ffi::CString::new(name.as_str())?;

    let status = unsafe {
        illumos::dladm_name2info(
            h,
            linkname.as_ptr(),
            &mut id as *mut illumos::datalink_id_t,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    if status != illumos::dladm_status_t_DLADM_STATUS_OK {
        return Err(Error::Dladm(format!("get link id for {}", name), status));
    }

    Ok(id)
}

pub(crate) fn destroy_vnic_interface(
    name: &String,
    h: *mut illumos::dladm_handle,
) -> Result<(), Error> {
    let id = match link_id(&name, h) {
        // link does not exist, nothing to do
        Err(Error::Dladm(_, illumos::dladm_status_t_DLADM_STATUS_NOTFOUND)) => return Ok(()),
        // for all other errors, pop up the stack
        Err(e) => return Err(e),
        // if the link is a think carry on with deleting it
        Ok(id) => id,
    };

    let flags = illumos::DLADM_OPT_ACTIVE | illumos::DLADM_OPT_PERSIST;

    let status = unsafe { illumos::dladm_vnic_delete(h, id, flags) };
    if status != illumos::dladm_status_t_DLADM_STATUS_OK {
        return Err(Error::Dladm(format!("delete vnic {}", name), status));
    }

    Ok(())
}

pub(crate) fn destroy_simnet_interface(
    name: &String,
    h: *mut illumos::dladm_handle,
) -> Result<(), Error> {
    let id = match link_id(&name, h) {
        // link does not exist, nothing to do
        Err(Error::Dladm(_, illumos::dladm_status_t_DLADM_STATUS_NOTFOUND)) => return Ok(()),
        // for all other errors, pop up the stack
        Err(e) => return Err(e),
        // if the link is a think carry on with deleting it
        Ok(id) => id,
    };
    let flags = illumos::DLADM_OPT_ACTIVE | illumos::DLADM_OPT_PERSIST;

    let status = unsafe { illumos::dladm_simnet_delete(h, id, flags) };
    if status != illumos::dladm_status_t_DLADM_STATUS_OK {
        return Err(Error::Dladm(format!("delete simnet {}", name), status));
    }

    Ok(())
}

pub(crate) fn connect_simnet_interfaces(
    x: &String,
    y: &String,
    h: *mut illumos::dladm_handle,
) -> Result<(), Error> {
    let xid = link_id(x, h)?;
    let yid = link_id(y, h)?;

    let flags = illumos::DLADM_OPT_ACTIVE | illumos::DLADM_OPT_PERSIST;

    let status = unsafe { illumos::dladm_simnet_modify(h, xid, yid, flags) };
    if status != illumos::dladm_status_t_DLADM_STATUS_OK {
        return Err(Error::Dladm(format!("modify simnet {}/{}", x, y), status));
    }

    Ok(())
}

pub(crate) fn create_vnic_interface(
    name: &String,
    simnet_link_id: illumos::datalink_id_t,
    h: *mut illumos::dladm_handle,
) -> Result<(), Error> {
    let linkname = ffi::CString::new(name.as_str())?;

    let mut mac_slot: os::raw::c_int = -1;
    let mut id: illumos::datalink_id_t = 0;
    let flags = illumos::DLADM_OPT_ACTIVE | illumos::DLADM_OPT_PERSIST;

    let status = unsafe {
        illumos::dladm_vnic_create(
            h,
            linkname.as_ptr(),
            simnet_link_id,
            illumos::vnic_mac_addr_type_t_VNIC_MAC_ADDR_TYPE_AUTO,
            ptr::null_mut(),
            0,
            &mut mac_slot as *mut os::raw::c_int,
            0,
            0,
            0,
            illumos::AF_UNSPEC as i32,
            &mut id as *mut illumos::datalink_id_t,
            ptr::null_mut(),
            flags,
        )
    };
    if status != illumos::dladm_status_t_DLADM_STATUS_OK {
        return Err(Error::Dladm(format!("create vnic {}", name), status));
    }

    Ok(())
}

pub(crate) fn create_simnet_interface(
    name: &String,
    h: *mut illumos::dladm_handle,
) -> Result<illumos::datalink_id_t, Error> {
    // create link
    let linkname = ffi::CString::new(name.as_str())?;
    let mtype = illumos::DL_ETHER;
    let flags = illumos::DLADM_OPT_ACTIVE | illumos::DLADM_OPT_PERSIST;

    let status = unsafe { illumos::dladm_simnet_create(h, linkname.as_ptr(), mtype, flags) };
    if status != illumos::dladm_status_t_DLADM_STATUS_OK {
        return Err(Error::Dladm(
            format!("create simnet {}", &linkname.to_str()?),
            status,
        ));
    }

    // return link id
    link_id(&name, h)
}
