//! Framing of application-owned integer words, separate from schema fields.
//!
//! `USFLD_SIZE` in the header declares how many integer words follow the
//! schema fields of a node. The public XT Format Reference (Siemens, April
//! 2008, printed p. 4, <https://ww3.cad.de/foren/ubb/uploads/schulze/XT_Format_April_2008_tcm73-62642.pdf>)
//! says the words follow nodes "visible at the PK interface" and tabulates
//! that visibility (pp. 115-117). Transmit files written by Parasolid 26 and
//! 27 (Siemens' own acceptance-test examples `disk_nat.x_t`, `sphere.x_t`,
//! `Kova.x_t` and `3DKovaGeom.xmt_txt`) append the words to every node,
//! including `ATT_DEF_ID` (79), `INT_VALUES` (82), `REAL_VALUES` (83) and
//! `CHAR_VALUES` (84), which that table lists as not visible: with the table
//! those files desynchronise at their first value node, with one word per node
//! they parse to their terminators and map complete B-Reps. The parser
//! therefore reads the declared words after every node. The words are retained
//! verbatim; no meaning is assigned to them.

use crate::ParseError;

/// Return the number of user words that follow one node.
///
/// # Errors
///
/// Returns `limits.exceeded` when the declared count exceeds the configured
/// variable-element limit.
pub(crate) fn word_count(
    declared_words: u8,
    offset: usize,
    element_limit: usize,
) -> Result<usize, ParseError> {
    let count = usize::from(declared_words);
    if count > element_limit {
        return Err(ParseError::limit(
            offset,
            "user_field_words",
            count,
            element_limit,
        ));
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ErrorKind;

    #[test]
    fn every_node_carries_the_declared_words() {
        assert_eq!(word_count(0, 7, 16), Ok(0));
        assert_eq!(word_count(1, 7, 16), Ok(1));
        assert_eq!(word_count(16, 0, 16), Ok(16));
    }

    #[test]
    fn declared_words_are_bounded_by_the_element_limit() {
        assert_eq!(word_count(1, 0, 1), Ok(1));
        assert_eq!(
            word_count(2, 0, 1).err().map(|error| error.kind()),
            Some(ErrorKind::LimitExceeded)
        );
        assert_eq!(word_count(0, 0, 0), Ok(0));
    }
}
