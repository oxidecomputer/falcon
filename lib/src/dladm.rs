// Copyright 2021 Oxide Computer Company

// import generated bindings
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(improper_ctypes)]
#![allow(dead_code)]
#![allow(deref_nullptr)]
#![allow(unaligned_references)]
include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

use crate::error::Error;
use std::{ffi, os, ptr};

//use libdladm_sys as dladm;

pub(crate) fn get_handle() -> Result<dladm_handle_t, Error> {
    let mut handle: dladm_handle_t = ptr::null_mut();
    let status = unsafe { dladm_open(&mut handle) };
    if status != dladm_status_t_DLADM_STATUS_OK {
        return Err(Error::Dladm(String::from("get handle"), status));
    }

    Ok(handle)
}

pub(crate) fn link_id(
    name: &String,
    h: *mut dladm_handle,
) -> Result<datalink_id_t, Error> {
    let mut id: datalink_id_t = 0;
    let linkname = ffi::CString::new(name.as_str())?;

    let status = unsafe {
        dladm_name2info(
            h,
            linkname.as_ptr(),
            &mut id as *mut datalink_id_t,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    if status != dladm_status_t_DLADM_STATUS_OK {
        return Err(Error::Dladm(format!("get link id for {}", name), status));
    }

    Ok(id)
}

pub(crate) fn destroy_vnic_interface(
    name: &String,
    h: *mut dladm_handle,
) -> Result<(), Error> {
    let id = match link_id(&name, h) {
        // link does not exist, nothing to do
        Err(Error::Dladm(_, dladm_status_t_DLADM_STATUS_NOTFOUND)) => {
            return Ok(())
        }
        // for all other errors, pop up the stack
        Err(e) => return Err(e),
        // if the link is a think carry on with deleting it
        Ok(id) => id,
    };

    let flags = DLADM_OPT_ACTIVE | DLADM_OPT_PERSIST;

    let status = unsafe { dladm_vnic_delete(h, id, flags) };
    if status != dladm_status_t_DLADM_STATUS_OK {
        return Err(Error::Dladm(format!("delete vnic {}", name), status));
    }

    Ok(())
}

pub(crate) fn destroy_simnet_interface(
    name: &String,
    h: *mut dladm_handle,
) -> Result<(), Error> {
    let id = match link_id(&name, h) {
        // link does not exist, nothing to do
        Err(Error::Dladm(_, dladm_status_t_DLADM_STATUS_NOTFOUND)) => {
            return Ok(())
        }
        // for all other errors, pop up the stack
        Err(e) => return Err(e),
        // if the link is a think carry on with deleting it
        Ok(id) => id,
    };
    let flags = DLADM_OPT_ACTIVE | DLADM_OPT_PERSIST;

    let status = unsafe { dladm_simnet_delete(h, id, flags) };
    if status != dladm_status_t_DLADM_STATUS_OK {
        return Err(Error::Dladm(format!("delete simnet {}", name), status));
    }

    Ok(())
}

pub(crate) fn connect_simnet_interfaces(
    x: &String,
    y: &String,
    h: *mut dladm_handle,
) -> Result<(), Error> {
    let xid = link_id(x, h)?;
    let yid = link_id(y, h)?;

    let flags = DLADM_OPT_ACTIVE | DLADM_OPT_PERSIST;

    let status = unsafe { dladm_simnet_modify(h, xid, yid, flags) };
    if status != dladm_status_t_DLADM_STATUS_OK {
        return Err(Error::Dladm(format!("modify simnet {}/{}", x, y), status));
    }

    Ok(())
}

pub(crate) fn create_vnic_interface(
    name: &String,
    simnet_link_id: datalink_id_t,
    h: *mut dladm_handle,
) -> Result<(), Error> {
    let linkname = ffi::CString::new(name.as_str())?;

    let mut mac_slot: os::raw::c_int = -1;
    let mut id: datalink_id_t = 0;
    let flags = DLADM_OPT_ACTIVE | DLADM_OPT_PERSIST;

    let status = unsafe {
        dladm_vnic_create(
            h,
            linkname.as_ptr(),
            simnet_link_id,
            vnic_mac_addr_type_t_VNIC_MAC_ADDR_TYPE_AUTO,
            ptr::null_mut(),
            0,
            &mut mac_slot as *mut os::raw::c_int,
            0,
            0,
            0,
            AF_UNSPEC as i32,
            &mut id as *mut datalink_id_t,
            ptr::null_mut(),
            flags,
        )
    };
    if status != dladm_status_t_DLADM_STATUS_OK {
        return Err(Error::Dladm(format!("create vnic {}", name), status));
    }

    Ok(())
}

pub(crate) fn create_simnet_interface(
    name: &String,
    h: *mut dladm_handle,
) -> Result<datalink_id_t, Error> {
    // create link
    let linkname = ffi::CString::new(name.as_str())?;
    let mtype = DL_ETHER;
    let flags = DLADM_OPT_ACTIVE | DLADM_OPT_PERSIST;

    let status =
        unsafe { dladm_simnet_create(h, linkname.as_ptr(), mtype, flags) };
    if status != dladm_status_t_DLADM_STATUS_OK {
        return Err(Error::Dladm(
            format!("create simnet {}", &linkname.to_str()?),
            status,
        ));
    }

    // return link id
    link_id(&name, h)
}
