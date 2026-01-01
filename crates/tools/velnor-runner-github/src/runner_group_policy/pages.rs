use std::collections::HashSet;

use crate::{SessionError, Transport, WireError};

use super::model::{CountPage, PageEntry};
use super::reader::get;
use crate::actions::{actions_request, status_error};
use crate::registration::{AsyncDiscoveryTransport, execute_discovery};

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
    let mut pages = PagesAccumulator::default();
    for page_number in 1..=MAX_PAGES {
        let query = format!("per_page={PAGE_SIZE}&page={page_number}");
        let response = get(transport, path, Some(query), actions_token)?;
        if response.status != 200 {
            return Err(status_error(response.status));
        }
        if pages.push(decode(&response.body)?)? {
            return Ok(pages.finish());
        }
    }
    Err(WireError::Malformed.into())
}

pub(super) async fn read_pages_async<T, Item, Decode>(
    transport: &mut T,
    path: &str,
    actions_token: &str,
    decode: Decode,
) -> Result<Vec<Item>, SessionError>
where
    T: AsyncDiscoveryTransport + ?Sized,
    Item: PageEntry,
    Decode: Fn(&[u8]) -> Result<CountPage<Item>, SessionError>,
{
    let mut pages = PagesAccumulator::default();
    for page_number in 1..=MAX_PAGES {
        let query = format!("per_page={PAGE_SIZE}&page={page_number}");
        let mut request = actions_request(path.to_owned(), actions_token)?;
        request.query = Some(query);
        let response = execute_discovery(transport, request).await?;
        if response.status != 200 {
            return Err(status_error(response.status));
        }
        if pages.push(decode(&response.body)?)? {
            return Ok(pages.finish());
        }
    }
    Err(WireError::Malformed.into())
}

struct PagesAccumulator<Item> {
    total: Option<usize>,
    items: Vec<Item>,
    seen_ids: HashSet<i64>,
    seen_names: HashSet<String>,
}

impl<Item> Default for PagesAccumulator<Item> {
    fn default() -> Self {
        Self {
            total: None,
            items: Vec::new(),
            seen_ids: HashSet::new(),
            seen_names: HashSet::new(),
        }
    }
}

impl<Item: PageEntry> PagesAccumulator<Item> {
    fn push(&mut self, page: CountPage<Item>) -> Result<bool, SessionError> {
        let count = page_total(page.total_count)?;
        if count > MAX_ITEMS || self.total.is_some_and(|known| known != count) {
            return Err(WireError::Malformed.into());
        }
        self.total = Some(count);
        let expected = count.saturating_sub(self.items.len()).min(PAGE_SIZE);
        if page.items.len() != expected {
            return Err(WireError::Malformed.into());
        }
        for item in &page.items {
            if !item.is_valid()
                || !self.seen_ids.insert(item.id())
                || !self.seen_names.insert(item.key().to_lowercase())
            {
                return Err(WireError::Malformed.into());
            }
        }
        self.items.extend(page.items);
        Ok(self.items.len() == count)
    }

    fn finish(self) -> Vec<Item> {
        self.items
    }
}

fn page_total(total: i64) -> Result<usize, SessionError> {
    usize::try_from(total).map_err(|_| WireError::Malformed.into())
}
