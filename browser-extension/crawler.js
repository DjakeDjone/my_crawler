const settings = {
    CRAWL_URL: 'https://api.search.fri3dl.dev/crawler/crawl',
    DEPTH: 1,
    MAX_PAGES: 12
}

const crawl = async (url) => {
    try {
        const response = await fetch(settings.CRAWL_URL, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
            },
            body: JSON.stringify(
                {
                    url,
                    depth: settings.DEPTH,
                    max_pages: settings.MAX_PAGES
                }),
        });

        if (!response.ok) {
            console.error('Crawl request failed:', response.statusText);
            return;
        }

        const data = await response.json();
        console.log('Crawl successful:', data);
    } catch (error) {
        console.error('Error during crawl request:', error);
    }
}

