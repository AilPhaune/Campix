pub enum Either<A, B> {
    A(A),
    B(B),
}

impl<A, B> Either<A, B> {
    pub fn new_left(a: A) -> Either<A, B> {
        Either::A(a)
    }

    pub fn new_right(b: B) -> Either<A, B> {
        Either::B(b)
    }

    pub fn map<C, D, F1, F2>(self, f1: F1, f2: F2) -> Either<C, D>
    where
        F1: FnOnce(A) -> C,
        F2: FnOnce(B) -> D,
    {
        match self {
            Either::A(a) => Either::A(f1(a)),
            Either::B(b) => Either::B(f2(b)),
        }
    }

    pub fn convert<C, F1, F2>(self, f1: F1, f2: F2) -> C
    where
        F1: FnOnce(A) -> C,
        F2: FnOnce(B) -> C,
    {
        match self {
            Either::A(a) => f1(a),
            Either::B(b) => f2(b),
        }
    }

    pub fn left_map<C, F>(self, f: F) -> Either<C, B>
    where
        F: FnOnce(A) -> C,
    {
        match self {
            Either::A(a) => Either::A(f(a)),
            Either::B(b) => Either::B(b),
        }
    }

    pub fn right_map<C, F>(self, f: F) -> Either<A, C>
    where
        F: FnOnce(B) -> C,
    {
        match self {
            Either::A(a) => Either::A(a),
            Either::B(b) => Either::B(f(b)),
        }
    }

    pub fn transpose(self) -> Either<B, A> {
        match self {
            Either::A(a) => Either::B(a),
            Either::B(b) => Either::A(b),
        }
    }

    pub fn referenced(&self) -> Either<&A, &B> {
        match self {
            Either::A(v) => Either::A(v),
            Either::B(v) => Either::B(v),
        }
    }

    pub fn get_left(self) -> Option<A> {
        match self {
            Either::A(v) => Some(v),
            _ => None,
        }
    }

    pub fn get_right(self) -> Option<B> {
        match self {
            Either::B(v) => Some(v),
            _ => None,
        }
    }
}

impl<T> Either<T, T> {
    pub fn get(self) -> T {
        match self {
            Either::A(v) | Either::B(v) => v,
        }
    }
}

impl<A: Clone, B: Clone> Clone for Either<A, B> {
    fn clone(&self) -> Self {
        match self {
            Either::A(a) => Either::A(a.clone()),
            Either::B(b) => Either::B(b.clone()),
        }
    }
}

impl<A: Copy, B: Copy> Copy for Either<A, B> {}

impl<A: PartialEq, B: PartialEq> PartialEq for Either<A, B> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Either::A(a), Either::A(b)) => a == b,
            (Either::B(a), Either::B(b)) => a == b,
            _ => false,
        }
    }
}

impl<A: Eq, B: Eq> Eq for Either<A, B> {}

impl<A, B> AsRef<Either<A, B>> for Either<A, B> {
    fn as_ref(&self) -> &Either<A, B> {
        self
    }
}

impl<A, B> AsMut<Either<A, B>> for Either<A, B> {
    fn as_mut(&mut self) -> &mut Either<A, B> {
        self
    }
}

impl<A: Clone, B: Clone> Either<&A, &B> {
    pub fn cloned(self) -> Either<A, B> {
        match self {
            Either::A(a) => Either::A(a.clone()),
            Either::B(b) => Either::B(b.clone()),
        }
    }
}

impl<A: Clone, B: Clone> Either<&A, &mut B> {
    pub fn cloned(self) -> Either<A, B> {
        match self {
            Either::A(a) => Either::A(a.clone()),
            Either::B(b) => Either::B(b.clone()),
        }
    }
}

impl<A: Clone, B: Clone> Either<&mut A, &B> {
    pub fn cloned(self) -> Either<A, B> {
        match self {
            Either::A(a) => Either::A(a.clone()),
            Either::B(b) => Either::B(b.clone()),
        }
    }
}

impl<A: Clone, B: Clone> Either<&mut A, &mut B> {
    pub fn cloned(self) -> Either<A, B> {
        match self {
            Either::A(a) => Either::A(a.clone()),
            Either::B(b) => Either::B(b.clone()),
        }
    }
}

impl<A: Copy, B: Copy> Either<&A, &B> {
    pub fn copied(self) -> Either<A, B> {
        match self {
            Either::A(a) => Either::A(*a),
            Either::B(b) => Either::B(*b),
        }
    }
}

impl<A: Copy, B: Copy> Either<&mut A, &B> {
    pub fn copied(self) -> Either<A, B> {
        match self {
            Either::A(a) => Either::A(*a),
            Either::B(b) => Either::B(*b),
        }
    }
}

impl<A: Copy, B: Copy> Either<&A, &mut B> {
    pub fn copied(self) -> Either<A, B> {
        match self {
            Either::A(a) => Either::A(*a),
            Either::B(b) => Either::B(*b),
        }
    }
}

impl<A: Copy, B: Copy> Either<&mut A, &mut B> {
    pub fn copied(self) -> Either<A, B> {
        match self {
            Either::A(a) => Either::A(*a),
            Either::B(b) => Either::B(*b),
        }
    }
}
