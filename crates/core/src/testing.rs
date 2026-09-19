use crate::model::{Identity, Locator, Occurrence, Part, Piece, Span};
use crate::reference::{BinderId, Reference, Site, Target};

pub fn occurrence(name: &str, parts: &[(Part, &str)]) -> Occurrence {
    Occurrence {
        locator: Locator {
            scope: Vec::new(),
            name: name.to_string(),
        },
        kind: "function".to_string(),
        file: "money.ts".to_string(),
        parts: parts
            .iter()
            .map(|(part, text)| (*part, vec![piece(text)]))
            .collect(),
        contract: None,
    }
}

pub fn piece(text: &str) -> Piece {
    Piece {
        text: text.to_string(),
        span: Span { start: 0, end: 0 },
        file: None,
    }
}

fn site(part: Part) -> Site {
    Site {
        part,
        span: Span { start: 0, end: 0 },
        found_by: BinderId("tsc".to_string()),
    }
}

/// A mention that is there in both snapshots.
pub fn reference(from: u32, to: u32, part: Part) -> Reference {
    Reference {
        from: Identity(from),
        to: Target::Known(Identity(to)),
        before: vec![site(part)],
        after: vec![site(part)],
    }
}
