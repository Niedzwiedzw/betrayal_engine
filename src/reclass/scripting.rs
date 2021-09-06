use std::convert::{TryFrom, TryInto};

use rhai::{Engine, EvalAltResult, Scope};

use crate::error::{BetrayalError, BetrayalResult};

macro_rules! constant {
    ($scope:expr, $field:ident) => {
        $scope.push_constant(
            format!("SIZE_{}", stringify!($field)),
            super::config_file::Field::$field.size() as i64,
        );
    };
    ($scope:expr, $name:literal, $size:expr) => {
        $scope.push_constant(format!("SIZE_{}", $name), $size as i64);
    };
}

fn try_cast_to_usize<T: 'static + TryInto<usize> + Clone + Copy>(
    engine: &Engine,
    mut scope: Scope,
    script: &str,
) -> BetrayalResult<usize> {
    let address: T = engine
        .eval_with_scope::<T>(&mut scope, script)
        .map_err(|e| BetrayalError::ScriptingError(e.to_string()))?
        .clone();
    Ok(address
        .try_into()
        .ok()
        .ok_or(BetrayalError::ScriptingError(format!(
            "failed to convert your value to your machine's `usize`"
        )))?)
}

macro_rules! or_err {
    ($val:expr, $mesg:expr) => {
        match $val {
            Ok(v) => v,
            Err(e) => return Err(format!("script error :: {} :: {}", $mesg, e).into()),
        }
    };
}

fn static_address(pid: i32, file: &str) -> Result<i64, Box<EvalAltResult>> {
    let maps = or_err!(crate::ProcessQuery::<u8>::mappings_all(pid), format!("static_address :: {}", file));
    let maps = maps.into_iter()
        .filter(|(_info, map)| match &map.pathname {
            procmaps::Path::MappedFile(s) => s == file && map.perms.writable == false,
            _ => false,
        }).collect::<Vec<_>>();
    if maps.len() > 1 {
        return Err(format!("static_address :: more than one memory entry with name [{}] found ({})", file, maps.len()).into());
    }

    let (_info, map) = maps.into_iter()
        .find(|(_info, map)| match &map.pathname {
            procmaps::Path::MappedFile(s) => s == file && map.perms.writable == false,
            _ => false,
        }).ok_or(format!("static_address() :: no such section : {}", file))?;

    // let (_info, map) = match maps
    //     .first() {
    //         Some(map) => map,
    //         None => return Err(format!("static_address :: {} :: no such map", file).into())
    //     };
    Ok(or_err!(map.base.try_into(), "that address doesn't fit in your address space"))
}


pub fn calculate_address(pid: i32, script: &str) -> BetrayalResult<usize> {
    let mut engine = Engine::new();
    let mut scope = Scope::new();
    // scope.push_constant(format!("SIZE_{}", "I32"), super::config_file::Field::I32.size());
    scope.push_constant("PID", pid);
    engine.register_result_fn("static_address", static_address);
    engine.on_print(|x| println!(" :: :: :: {}", x));
    constant!(scope, I32);
    constant!(scope, I16);
    constant!(scope, U8);
    constant!(scope, F32);
    constant!(scope, F64);

    try_cast_to_usize::<i16>(&engine, scope.clone(), script)
        .or(try_cast_to_usize::<i32>(&engine, scope.clone(), script))
        .or(try_cast_to_usize::<i64>(&engine, scope.clone(), script))
        .or(try_cast_to_usize::<usize>(&engine, scope.clone(), script))
}
