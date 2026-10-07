use std::collections::HashSet;

use crate::{SessionError, Transport, WireError};

use super::model::{CountPage, PageEntry};
use super::reader::get;
use crate::actions::status_error;

const PAGE_SIZE: usize = 100;
const MAX_PAGES: usize = 32;
const MAX_ITEMS: usize = PAGE_SIZE * MAX_PAGES;

pub(super) fn read_pages<T, Item, Decode>(
    transport: &mut T,
    path: &str,
    actions_token: &str,
    decode: Decode,
) -> Result<Vec<Item>, SessionError>
where
    T: Transport + ?Sized,
    Item: PageEntry,
    Decode: Fn(&[u8]) -> Result<CountPage<Item>, SessionError>,
{
    let mut total = None;
    let mut items = Vec::new();
    let mut seen_ids = HashSet::new();
    let mut seen_names = HashSet::new();
    for page_number in 1..=MAX_PAGES {
        let query = format!("per_page={PAGE_SIZE}&page={page_number}");
        let response = get(transport, path, Some(query), actions_token)?;
        if response.status != 200 {
            return Err(status_error(response.status));
        }
        let page = decode(&response.body)?;
        let count = page_total(page.total_count)?;
        if count > MAX_ITEMS || total.is_some_and(|known| known != count) {
            return Err(WireError::Malformed.into());
        }
        total = Some(count);
        let expected = (count - items.len()).min(PAGE_SIZE);
        if page.items.len() != expected {
            return Err(WireError::Malformed.into());
        }
        for item in &page.items {
            if !item.is_valid()
                || !seen_ids.insert(item.id())
                || !seen_names.insert(item.key().to_lowercase())
            {
                return Err(WireError::Malformed.into());
            }
        }
        items.extend(page.items);
        if items.len() == count {
            return Ok(items);
        }
    }
    Err(WireError::Malformed.into())
}

fn page_total(total: i64) -> Result<usize, SessionError> {
    usize::try_from(total).map_err(|_| WireError::Malformed.into())
}
