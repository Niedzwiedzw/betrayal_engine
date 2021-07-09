use crate::{AddressValue, ProcessQuery, error::{BetrayalError, BetrayalResult}, memory::ReadFromBytes};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NeighbourValuesQuery<T: ReadFromBytes> {
    pub window_size: usize,
    pub values: Vec<T>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NeighbourValues<T: ReadFromBytes> {
    pub window_size: usize,
    pub values: Vec<AddressValue<T>>,
}
