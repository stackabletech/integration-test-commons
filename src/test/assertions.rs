//! Additional assertions for [`spectral`]

use spectral::{iter::ContainingIntoIterAssertions, vec::VecAssertions, AssertionFailure, Spec};
use std::fmt::Debug;

/// Additional assertions for Vec
pub trait ExtendedVecAssertions<'s, T: 's> {
    fn is_not_empty(&self);
    fn contains_exactly_in_any_order<E: 's>(&mut self, expected_values_iter: &'s E)
    where
        E: IntoIterator<Item = &'s T> + Clone,
        E::IntoIter: ExactSizeIterator;
}

impl<'s, T: 's> ExtendedVecAssertions<'s, T> for Spec<'s, Vec<T>>
where
    T: PartialEq + Debug,
{
    /// Asserts that the subject vector is not empty.
    ///
    /// ```rust
    /// let test_vec: Vec<u8> = vec![1];
    /// assert_that(&test_vec).is_not_empty();
    /// ```
    fn is_not_empty(&self) {
        if self.subject.is_empty() {
            AssertionFailure::from_spec(self)
                .with_expected(String::from("a non-empty vec"))
                .with_actual(String::from("an empty vec"))
                .fail();
        }
    }

    /// Asserts that the subject vector contains exactly the provided values in any order.
    ///
    /// ```rust
    /// let test_vec = vec![1, 2, 3];
    /// assert_that(&test_vec).contains_exactly_in_any_order(&vec![&3, &1, &2]);
    /// ```
    fn contains_exactly_in_any_order<E: 's>(&mut self, expected_values_iter: &'s E)
    where
        E: IntoIterator<Item = &'s T> + Clone,
        E::IntoIter: ExactSizeIterator,
    {
        self.has_length(expected_values_iter.clone().into_iter().len());
        self.contains_all_of(expected_values_iter);
    }
}
