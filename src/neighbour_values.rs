use crate::{AddressValue, ProcessQuery, error::{BetrayalError, BetrayalResult}};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NeighbourValuesQuery {
    pub window_size: usize,
    pub values: Vec<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NeighbourValues {
    pub window_size: usize,
    pub values: Vec<AddressValue<i32>>,
}
