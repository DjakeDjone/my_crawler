const settings = {
    CRAWL_URL: 'https://api.search.fri3dl.dev/crawler/crawl',
    MAX_DEPTH: 1,
    MAX_PAGES: 12
}

const crawl = async (url, maxDepth, maxPages) => {
    try {
        const response = await fetch(settings.CRAWL_URL, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
            },
            body: JSON.stringify({
                url,
                max_depth: maxDepth || settings.MAX_DEPTH,
                max_pages: maxPages || settings.MAX_PAGES
            }),
        });

        if (!response.ok) {
            console.error('Crawl request failed:', response.statusText);
            return { success: false, error: response.statusText };
        }

        const data = await response.json();
        console.log('Crawl successful:', data);
        return { success: true, data };
    } catch (error) {
        console.error('Error during crawl request:', error);
        return { success: false, error: error.message };
    }
}

// Get current tab URL and populate input
// Manifest V3 compatible
chrome.tabs.query({ active: true, currentWindow: true }).then(tabs => {
    const currentTab = tabs[0];
    if (currentTab && currentTab.url) {
        document.getElementById('url-input').value = currentTab.url;
    }
}).catch(error => {
    console.error('Error getting current tab:', error);
});

// Handle crawl button click
document.getElementById('crawl-btn').addEventListener('click', async () => {
    const urlInput = document.getElementById('url-input');
    const maxDepthInput = document.getElementById('max-depth');
    const maxPagesInput = document.getElementById('max-pages');
    const statusDiv = document.getElementById('status');
    const crawlBtn = document.getElementById('crawl-btn');

    const url = urlInput.value.trim();
    
    if (!url) {
        statusDiv.textContent = 'Please enter a URL';
        statusDiv.style.color = 'red';
        return;
    }

    // Validate URL
    try {
        new URL(url);
    } catch (e) {
        statusDiv.textContent = 'Please enter a valid URL';
        statusDiv.style.color = 'red';
        return;
    }

    // Disable button and show loading state
    crawlBtn.disabled = true;
    crawlBtn.textContent = 'Crawling...';
    statusDiv.textContent = 'Starting crawl...';
    statusDiv.style.color = 'var(--text)';

    // Perform crawl
    const maxDepth = parseInt(maxDepthInput.value) || settings.MAX_DEPTH;
    const maxPages = parseInt(maxPagesInput.value) || settings.MAX_PAGES;
    
    const result = await crawl(url, maxDepth, maxPages);

    // Re-enable button
    crawlBtn.disabled = false;
    crawlBtn.textContent = 'Crawl';

    // Show result
    if (result.success) {
        statusDiv.textContent = 'Crawl queued successfully!';
        statusDiv.classList.remove('error');
        statusDiv.classList.add('success');
    } else {
        statusDiv.textContent = `Crawl failed: ${result.error}`;
        statusDiv.classList.remove('success');
        statusDiv.classList.add('error');
    }
});
