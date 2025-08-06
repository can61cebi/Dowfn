const NATIVE_HOST = 'com.dowfn.native';
let nativePort = null;
let isConnected = false;

function connectNative() {
    try {
        nativePort = chrome.runtime.connectNative(NATIVE_HOST);
        
        nativePort.onMessage.addListener((message) => {
            console.log('Received from native:', message);
            handleNativeMessage(message);
        });
        
        nativePort.onDisconnect.addListener(() => {
            console.log('Native messaging disconnected');
            isConnected = false;
            nativePort = null;
            
            if (chrome.runtime.lastError) {
                console.error('Native messaging error:', chrome.runtime.lastError.message);
            }
            
            setTimeout(connectNative, 5000);
        });
        
        isConnected = true;
        console.log('Connected to native host');
        
        sendHandshake();
    } catch (error) {
        console.error('Failed to connect to native host:', error);
        isConnected = false;
        setTimeout(connectNative, 5000);
    }
}

function sendHandshake() {
    sendToNative({
        type: 'Handshake',
        payload: {
            version: '0.1.0',
            client_id: chrome.runtime.id,
            capabilities: ['downloads', 'contextMenu', 'intercept']
        }
    });
}

function sendToNative(message) {
    if (nativePort && isConnected) {
        try {
            nativePort.postMessage(message);
        } catch (error) {
            console.error('Failed to send message to native:', error);
            connectNative();
        }
    } else {
        console.warn('Native port not connected, queuing message');
        connectNative();
    }
}

function handleNativeMessage(message) {
    switch (message.type) {
        case 'Progress':
            updateDownloadProgress(message.data);
            break;
        case 'StatusChange':
            updateDownloadStatus(message.data);
            break;
        case 'Success':
            showNotification('Success', message.data.message);
            break;
        case 'Error':
            showNotification('Error', message.data.message, 'error');
            break;
    }
}

chrome.downloads.onCreated.addListener((downloadItem) => {
    chrome.storage.sync.get(['interceptDownloads', 'minSize'], (settings) => {
        if (settings.interceptDownloads !== false) {
            const minSize = settings.minSize || 1048576;
            
            if (!downloadItem.fileSize || downloadItem.fileSize > minSize) {
                chrome.downloads.cancel(downloadItem.id, () => {
                    sendDownloadToNative(downloadItem);
                });
            }
        }
    });
});

function sendDownloadToNative(downloadItem) {
    const request = {
        url: downloadItem.url,
        file_name: downloadItem.filename ? downloadItem.filename.split('/').pop() : null,
        save_path: null,
        threads: null,
        metadata: {
            referrer: downloadItem.referrer || null,
            user_agent: navigator.userAgent,
            cookies: null,
            auto_start: true
        }
    };
    
    sendToNative({
        type: 'Command',
        payload: {
            type: 'AddDownload',
            data: request
        }
    });
}

chrome.contextMenus.create({
    id: 'download-with-dowfn',
    title: 'Download with Dowfn',
    contexts: ['link', 'image', 'video', 'audio']
});

chrome.contextMenus.onClicked.addListener((info, tab) => {
    if (info.menuItemId === 'download-with-dowfn') {
        const url = info.linkUrl || info.srcUrl || info.pageUrl;
        
        if (url) {
            sendDownloadToNative({
                url: url,
                referrer: tab.url
            });
        }
    }
});

chrome.runtime.onMessage.addListener((request, sender, sendResponse) => {
    switch (request.action) {
        case 'getStatus':
            sendResponse({ connected: isConnected });
            break;
        case 'addDownload':
            sendToNative({
                type: 'Command',
                payload: {
                    type: 'AddDownload',
                    data: request.data
                }
            });
            sendResponse({ success: true });
            break;
        case 'getDownloads':
            sendToNative({
                type: 'Command',
                payload: {
                    type: 'GetAllDownloads'
                }
            });
            break;
    }
    return true;
});

function showNotification(title, message, type = 'info') {
    const iconUrl = type === 'error' ? 'icons/error.png' : 'icons/icon48.png';
    
    chrome.notifications.create({
        type: 'basic',
        iconUrl: iconUrl,
        title: title,
        message: message
    });
}

function updateDownloadProgress(data) {
    chrome.runtime.sendMessage({
        action: 'progressUpdate',
        data: data
    }).catch(() => {});
}

function updateDownloadStatus(data) {
    chrome.runtime.sendMessage({
        action: 'statusUpdate',
        data: data
    }).catch(() => {});
}

connectNative();