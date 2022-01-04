//! All of this is cool but ansi_term does it better.

/// Defines ANSI escape codes for convenience.
// See also
// https://en.wikipedia.org/wiki/ANSI_escape_code#Escape_sequences
// Just need to use `\u{1b}` rather than `\033` for the ESC
use std::ops::Range;
use std::string::String;

/// Cease all formatting
pub const END: &str = "\u{1b}[0m";

/// Bold text
pub const BOLD: &str = "\u{1b}[1m";

/// "Faint" text
pub const FAINT: &str = "\u{1b}[2m";

/// Italic text
pub const ITALIC: &str = "\u{1b}[2m";

/// Underlined text
pub const UNDERLINE: &str = "\u{1b}[4m";

/// Swap foreground and background colors
pub const NEGATIVE: &str = "\u{1b}[7m";

/// Cursor Previous Line
// pub const CPL: &str = "\u{1b}[F";

/// Erase In Line
// pub const EIL: &str = "\u{1b}[2K";

/// Non-standard: a CPL followed by EIL
/// Preface println with this to simply overwrite the previous line
pub const TTYJUMP: &str = "\u{1b}[F\u{1b}[2K";

// pub const ALL_SYMBOLS: [&str; 7] = [END, BOLD, FAINT, ITALIC, UNDERLINE, NEGATIVE, TTYJUMP];

const SPAN_SYMBOLS: [&str; 5] = [BOLD, FAINT, ITALIC, UNDERLINE, NEGATIVE];

/// Decorate a string by prepending the relevant ANSI control code
/// and appending the ANSI stop-formatting code. Retains any existing
/// formatting too, including that which would continue beyond the
/// end of the input string.
pub fn decorate(input: &str, which: &str) -> String {
    // Split by END. For each segment, simply append the decoration
    // and then append `END`. Except for the last/only segment, in
    // which case append any other decorations after the END.

    // Within each split, also initially split by which,
    // then recombine without it. This ensures no (less) duplication.

    let split_enz: Vec<&str> = input.split(END).collect();
    let mut output = String::with_capacity(input.len() + (2 * split_enz.len())); // both those are in bytes

    for s in split_enz.iter() {
        output.push_str(which);
        output.push_str(&s.split(&which).collect::<Vec<&str>>().concat());
        output.push_str(END);
    }

    for d in SPAN_SYMBOLS.iter() {
        if split_enz[split_enz.len() - 1].contains(d) {
            output.push_str(d);
        }
    }
    return output;
}

/// Applies a decoration to part of a string
/// Panics if `range` is wonky.
pub fn decorate_range(input: &str, range: Range<usize>, which: &str) -> String {
    let (initial, midend) = input.split_at(range.start);
    let (middle, last) = midend.split_at(range.end - range.start);
    let mut output = String::with_capacity(input.len() + 2);

    output.push_str(initial);
    output.push_str(&decorate(middle, which));

    if middle.split(END).count() == 1 {
        // No ENDs in middle
        // It's possible that some formatting was started in initial
        // and therefore needs to be propogated.
        let startz: Vec<&str> = initial.rsplitn(2, END).collect();
        // startz[0] will be what we need.

        for d in SPAN_SYMBOLS.iter() {
            if startz[0].contains(d) {
                output.push_str(d);
            }
        }
    }

    output.push_str(last);

    return output;
}
