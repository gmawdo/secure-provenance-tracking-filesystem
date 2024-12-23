use crate::kernel::protocol::context::RPCContext;
use crate::kernel::api::mount::*;
use crate::kernel::protocol::rpc::*;
use crate::kernel::protocol::xdr::*;
use num_derive::{FromPrimitive, ToPrimitive};
use num_traits::cast::{FromPrimitive, ToPrimitive};
use std::io::{Read, Write};
use tracing::debug;

use anyhow::Result;

use crate::backingstore::data_store::KeyType;

#[allow(non_camel_case_types)]
#[allow(clippy::upper_case_acronyms)]
#[derive(Copy, Clone, Debug, FromPrimitive, ToPrimitive)]
enum MountProgram {
    MOUNTPROC3_NULL = 0,
    MOUNTPROC3_MNT = 1,
    MOUNTPROC3_DUMP = 2,
    MOUNTPROC3_UMNT = 3,
    MOUNTPROC3_UMNTALL = 4,
    MOUNTPROC3_EXPORT = 5,
    INVALID,
}

pub async fn handle_mount(
    xid: u32,
    call: call_body,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    let prog = MountProgram::from_u32(call.proc).unwrap_or(MountProgram::INVALID);

    match prog {
        MountProgram::MOUNTPROC3_NULL => mountproc3_null(xid, input, output)?,
        MountProgram::MOUNTPROC3_MNT => mountproc3_mnt(xid, input, output, context).await?,
        MountProgram::MOUNTPROC3_UMNT => mountproc3_umnt(xid, input, output, context).await?,
        MountProgram::MOUNTPROC3_UMNTALL => {
            mountproc3_umnt_all(xid, input, output, context).await?
        }
        MountProgram::MOUNTPROC3_EXPORT => mountproc3_export(xid, input, output)?,
        _ => {
            proc_unavail_reply_message(xid).serialize(output)?;
        }
    }
    Ok(())
}

pub fn mountproc3_null(
    xid: u32,
    _: &mut impl Read,
    output: &mut impl Write,
) -> Result<(), anyhow::Error> {
    debug!("mountproc3_null({:?}) ", xid);
    // build an RPC reply
    let msg = make_success_reply(xid);
    debug!("\t{:?} --> {:?}", xid, msg);
    msg.serialize(output)?;
    Ok(())
}

#[allow(non_camel_case_types)]
#[derive(Clone, Debug)]
pub struct mountres3_ok {
    pub fhandle: fhandle3,
    pub auth_flavors: Vec<u32>,
}
XDRStruct!(mountres3_ok, fhandle, auth_flavors);

pub async fn mountproc3_mnt(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    debug!("=== Handling MOUNTPROC3_MNT request ===");
    debug!("=== XID: {} ===", xid);
    
    let mut path = dirpath::new();
    path.deserialize(input)?;
    
    let path_str = std::str::from_utf8(&path).unwrap_or_default();
    debug!("=== Mount path received: {} ===", path_str);
    
    let mut user_key = None;
    let options: Vec<&str> = path_str.split('/').collect();

    for option in options {
        if option.ends_with("'s drive") {
            user_key = Some(option.trim_end_matches("'s drive").to_string());
        }
    }

    let utf8path: String = if let Some(ref user_key) = user_key {
        match context.vfs.data_store().authenticate_user(user_key).await {
            KeyType::Usual => {
                println!("Authenticated as a standard user: {}", user_key);
                format!("/{}", user_key)
            }
            KeyType::Special => {
                println!("Authenticated as a superuser: {}", user_key);
                String::from("/")
            }
            KeyType::None => {
                make_failure_reply(xid).serialize(output)?;
                return Err(anyhow::anyhow!("Authentication failed"));
            }
        }
    } else {
        make_failure_reply(xid).serialize(output)?;
        return Err(anyhow::anyhow!("User key not provided"));
    };

    context.vfs.data_store().init_user_directory(&utf8path).await.map_err(|_| {
        let _ = make_failure_reply(xid).serialize(output);
        anyhow::anyhow!("Failed to initialize user directory")
    })?;

    debug!("mountproc3_mnt({:?},{:?}) ", xid, utf8path);
    if let Ok(fileid) = context.vfs.get_id_from_path(&utf8path, context.vfs.data_store()).await {
        let response = mountres3_ok {
            fhandle: context.vfs.id_to_fh(fileid).data,
            auth_flavors: vec![
                auth_flavor::AUTH_NULL.to_u32().unwrap(),
                auth_flavor::AUTH_UNIX.to_u32().unwrap(),
            ],
        };
        debug!("{:?} --> {:?}", xid, response);

        if let Some(ref chan) = context.mount_signal {
            let _ = chan.send(true).await;
        }
        make_success_reply(xid).serialize(output)?;
        mountstat3::MNT3_OK.serialize(output)?;
        response.serialize(output)?;
    } else {
        debug!("{:?} --> MNT3ERR_NOENT", xid);
        make_success_reply(xid).serialize(output)?;
        mountstat3::MNT3ERR_NOENT.serialize(output)?;
    }
    Ok(())
}


/*

DESCRIPTION

  Procedure EXPORT returns a list of all the exported file
  systems and which clients are allowed to mount each one.
  The names in the group list are implementation-specific
  and cannot be directly interpreted by clients. These names
  can represent hosts or groups of hosts.

IMPLEMENTATION

  This procedure generally returns the contents of a list of
  shared or exported file systems. These are the file
  systems which are made available to NFS version 3 protocol
  clients.
 */

pub fn mountproc3_export(
    xid: u32,
    _: &mut impl Read,
    output: &mut impl Write,
) -> Result<(), anyhow::Error> {
    debug!("mountproc3_export({:?}) ", xid);
    make_success_reply(xid).serialize(output)?;
    true.serialize(output)?;
    // dirpath
    "/".as_bytes().to_vec().serialize(output)?;
    // groups
    false.serialize(output)?;
    // next exports
    false.serialize(output)?;
    Ok(())
}

pub async fn mountproc3_umnt(
    xid: u32,
    input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    let mut path = dirpath::new();
    path.deserialize(input)?;
    let utf8path = std::str::from_utf8(&path).unwrap_or_default();
    debug!("mountproc3_umnt({:?},{:?}) ", xid, utf8path);
    if let Some(ref chan) = context.mount_signal {
        let _ = chan.send(false).await;
    }
    make_success_reply(xid).serialize(output)?;
    mountstat3::MNT3_OK.serialize(output)?;
    Ok(())
}

pub async fn mountproc3_umnt_all(
    xid: u32,
    _input: &mut impl Read,
    output: &mut impl Write,
    context: &RPCContext,
) -> Result<(), anyhow::Error> {
    debug!("mountproc3_umnt_all({:?}) ", xid);
    if let Some(ref chan) = context.mount_signal {
        let _ = chan.send(false).await;
    }
    make_success_reply(xid).serialize(output)?;
    mountstat3::MNT3_OK.serialize(output)?;
    Ok(())
}



