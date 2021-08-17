use std::convert::{TryFrom, TryInto};

use rhai::{Engine, Scope};

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

pub fn calculate_address(script: &str) -> BetrayalResult<usize> {
    let engine = Engine::new();
    let mut scope = Scope::new();
    // scope.push_constant(format!("SIZE_{}", "I32"), super::config_file::Field::I32.size());
    constant!(scope, I32);
    constant!(scope, I16);
    constant!(scope, U8);
    constant!(scope, F32);
    constant!(scope, F64);
    constant!(scope, "POINTER", std::mem::size_of::<usize>());
    try_cast_to_usize::<i16>(&engine, scope.clone(), script)
        .or(try_cast_to_usize::<i32>(&engine, scope.clone(), script))
        .or(try_cast_to_usize::<i64>(&engine, scope.clone(), script))
        .or(try_cast_to_usize::<usize>(&engine, scope.clone(), script))
}
