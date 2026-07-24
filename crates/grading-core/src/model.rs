use alloc::vec::Vec;

/// A 0-based option index into a multiple-choice question (A=0, B=1, ...).
pub type Choice = u8;

/// Sentinel used in canonical encodings for "left blank". Never a valid
/// [`Choice`], because `num_choices <= 255` implies valid choices are `0..=254`.
pub(crate) const BLANK_MARKER: u8 = 0xFF;

/// What happens to a question that was cancelled after the exam (a routine
/// event in real exams: a flawed question is struck by the appeals board).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelPolicy {
    /// Every student receives the cancelled question's weight as credit.
    FullCredit = 0,
    /// The cancelled question is removed and remaining weights are
    /// renormalized (integer math, floor division).
    Redistribute = 1,
}

/// One question's entry in the (private) answer key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyEntry {
    pub question_id: u32,
    /// Relative weight; scores are computed as weighted basis points, so
    /// weights need not sum to anything in particular.
    pub weight: u32,
    /// Accepted choices, sorted ascending, deduplicated. More than one entry
    /// is how "answer B was also ruled correct on appeal" is represented.
    /// Must be non-empty unless `cancelled`.
    pub accepted: Vec<Choice>,
    pub cancelled: bool,
}

/// The private input of the system: the answer key. Its canonical hash with a
/// salt is published *before* the exam as a commitment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnswerKey {
    pub exam_id: u64,
    /// Number of options per question (uniform across the exam in v0).
    pub num_choices: u8,
    pub cancel_policy: CancelPolicy,
    /// One entry per question, in exam order. Question ids must be unique.
    pub entries: Vec<KeyEntry>,
}

/// One student's submitted answers. Contains no direct PII: the student is
/// identified by an opaque 32-byte pseudonym (e.g. a hash derived by the
/// institution) so that answer sheets can be published or shared for audit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnswerSheet {
    pub exam_id: u64,
    pub student_pseudonym: [u8; 32],
    /// One slot per question, in the same order as the key. `None` = blank.
    pub answers: Vec<Option<Choice>>,
}

/// The public outcome of grading one sheet against one key. Everything here
/// is safe to publish next to a proof.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScoreReport {
    pub exam_id: u64,
    /// Commitment to the answer key the score was computed against.
    pub key_commitment: [u8; 32],
    /// Canonical hash of the answer sheet that was graded.
    pub sheet_hash: [u8; 32],
    /// Final score in basis points, 0..=10_000 (i.e. 8_250 = 82.5%).
    pub score_bp: u32,
    pub correct: u32,
    pub wrong: u32,
    pub blank: u32,
    pub cancelled: u32,
}
