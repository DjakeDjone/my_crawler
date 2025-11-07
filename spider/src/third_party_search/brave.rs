use crate::third_party_search::ThirdPartySearch;

pub struct BraveSearch;

impl ThirdPartySearch for BraveSearch {
    fn search_for(query: &str, result_count: usize) -> Vec<String> {
        todo!()
    }
}
