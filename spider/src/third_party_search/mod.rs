pub mod brave;

pub trait ThirdPartySearch {
    fn search_for(query: &str, result_count: usize) -> Vec<String>; // returns vector of urls
}
