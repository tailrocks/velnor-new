#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterRef {
    All,
    AllExcept(Vec<String>),
    Expr {
        expr: String,
        except: Vec<String>,
    },
    Group {
        include: Option<String>,
        exclude: Vec<String>,
        except: Vec<String>,
    },
    Excluding {
        base: Box<FilterRef>,
        lanes: Vec<String>,
    },
}

impl FilterRef {
    #[must_use]
    pub fn excluding(self, lanes: Vec<String>) -> Self {
        if lanes.is_empty() {
            self
        } else {
            Self::Excluding {
                base: Box::new(self),
                lanes,
            }
        }
    }
}
