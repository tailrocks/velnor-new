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
}
