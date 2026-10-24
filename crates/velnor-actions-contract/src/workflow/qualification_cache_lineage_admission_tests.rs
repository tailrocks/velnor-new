use std::error::Error;

#[path = "qualification_cache_lineage_admission_fixture.rs"]
mod admission_fixture;
use super::*;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

const SOURCE_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SOURCE_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[path = "qualification_cache_lineage_admission_tests/campaigns.rs"]
mod campaigns;
#[path = "qualification_cache_lineage_admission_tests/layers.rs"]
mod layers;
#[path = "qualification_cache_lineage_admission_tests/rejections.rs"]
mod rejections;
