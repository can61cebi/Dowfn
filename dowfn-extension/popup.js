document.addEventListener('DOMContentLoaded', () => {
    const statusDot = document.getElementById('statusDot');
    const statusText = document.getElementById('statusText');
    const urlInput = document.getElementById('urlInput');
    const addBtn = document.getElementById('addBtn');
    const pasteBtn = document.getElementById('pasteBtn');
    const downloadsList = document.getElementById('downloadsList');
    const settingsLink = document.getElementById('settingsLink');
    
    let downloads = new Map();
    
    function updateConnectionStatus() {
        chrome.runtime.sendMessage({ action: 'getStatus' }, (response) => {
            if (response && response.connected) {
                statusDot.classList.add('connected');
                statusText.textContent = 'Connected';
                addBtn.disabled = false;
            } else {
                statusDot.classList.remove('connected');
                statusText.textContent = 'Disconnected';
                addBtn.disabled = true;
            }
        });
    }
    
    function addDownload() {
        const url = urlInput.value.trim();
        
        if (!url) {
            return;
        }
        
        try {
            new URL(url);
        } catch {
            alert('Please enter a valid URL');
            return;
        }
        
        chrome.runtime.sendMessage({
            action: 'addDownload',
            data: {
                url: url,
                auto_start: true
            }
        }, (response) => {
            if (response && response.success) {
                urlInput.value = '';
                loadDownloads();
            }
        });
    }
    
    function loadDownloads() {
        chrome.runtime.sendMessage({ action: 'getDownloads' });
    }
    
    function renderDownloads() {
        if (downloads.size === 0) {
            downloadsList.innerHTML = '<div class="empty-state">No active downloads</div>';
            return;
        }
        
        let html = '';
        
        downloads.forEach((download) => {
            const progress = download.total_size > 0 
                ? (download.downloaded_size / download.total_size * 100).toFixed(1)
                : 0;
            
            html += `
                <div class="download-item">
                    <div class="download-name">${download.file_name}</div>
                    <div class="download-progress">
                        <div class="download-progress-bar" style="width: ${progress}%"></div>
                    </div>
                    <div class="download-info">
                        <span>${formatBytes(download.downloaded_size)} / ${formatBytes(download.total_size || 0)}</span>
                        <span>${download.status}</span>
                    </div>
                </div>
            `;
        });
        
        downloadsList.innerHTML = html;
    }
    
    function formatBytes(bytes) {
        if (bytes === 0) return '0 B';
        
        const k = 1024;
        const sizes = ['B', 'KB', 'MB', 'GB'];
        const i = Math.floor(Math.log(bytes) / Math.log(k));
        
        return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
    }
    
    addBtn.addEventListener('click', addDownload);
    
    urlInput.addEventListener('keypress', (e) => {
        if (e.key === 'Enter') {
            addDownload();
        }
    });
    
    pasteBtn.addEventListener('click', async () => {
        try {
            const text = await navigator.clipboard.readText();
            urlInput.value = text.trim();
        } catch (err) {
            console.error('Failed to read clipboard:', err);
        }
    });
    
    settingsLink.addEventListener('click', (e) => {
        e.preventDefault();
        chrome.runtime.openOptionsPage();
    });
    
    chrome.runtime.onMessage.addListener((message) => {
        switch (message.action) {
            case 'progressUpdate':
                if (message.data && downloads.has(message.data.download_id)) {
                    const download = downloads.get(message.data.download_id);
                    download.downloaded_size = message.data.downloaded;
                    renderDownloads();
                }
                break;
            case 'statusUpdate':
                if (message.data && downloads.has(message.data.download_id)) {
                    const download = downloads.get(message.data.download_id);
                    download.status = message.data.new_status;
                    renderDownloads();
                }
                break;
            case 'downloadsList':
                downloads.clear();
                if (message.data && Array.isArray(message.data)) {
                    message.data.forEach(d => {
                        downloads.set(d.id, d);
                    });
                }
                renderDownloads();
                break;
        }
    });
    
    updateConnectionStatus();
    loadDownloads();
    
    setInterval(updateConnectionStatus, 5000);
});