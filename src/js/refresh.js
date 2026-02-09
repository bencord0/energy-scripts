if ('caches' in window) {
    const cacheName = 'octopus-data-v1';
    const url = '/data/power.sqlite3';

    const cache = await caches.open(cacheName);
    await cache.delete(url);

}

window.location.replace('/');
