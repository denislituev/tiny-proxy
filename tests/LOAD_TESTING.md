# Load Testing with k6

## Prerequisites

1. Install k6:
```bash
# macOS
brew install k6

# Linux
sudo apt-get install k6

# Docker
docker pull grafana/k6
```

2. Make sure proxy and backends are running:
```bash
# Terminal 1 - Proxy
cargo run

# Terminal 2 - Backend 1
~/go/bin/http-echo -listen=:9001 -text="Hello from User Service"

# Terminal 3 - Backend 2
~/go/bin/http-echo -listen=:9002 -text="Hello from Order Service"
```

## Running Load Tests

### Quick Test (small load)
```bash
k6 run tests/load-test.js
```

### Custom Load Test

Edit `tests/load-test.js` to adjust load parameters:

```javascript
export const options = {
    stages: [
        { duration: '30s', target: 10 },   // Ramp up to 10 users
        { duration: '1m', target: 50 },    // Stay at 50 users
        { duration: '30s', target: 100 },  // Ramp up to 100 users
        { duration: '1m', target: 100 },  // Stay at 100 users
        { duration: '30s', target: 0 },    // Ramp down to 0
    ],
};
```

### Run with Output File
```bash
k6 run --out json=results.json tests/load-test.js
```

### Run with Docker
```bash
docker run --network=host grafana/k6 run tests/load-test.js
```

## Understanding Results

### Key Metrics

- **http_req_duration**: Request latency (p(95) means 95th percentile)
- **http_reqs**: Total number of requests
- **http_req_failed**: Failed requests
- **vus**: Virtual users (concurrent load)
- **rps**: Requests per second

### Sample Output
```
✓ proxy-users status is 200
✓ proxy-users response time < 500ms
✓ proxy-users has body

checks.........................: 100.00% ✓ 5000       ✗ 0
data_received..................: 2.1 MB  11 kB/s
data_sent....................: 520 kB  2.7 kB/s
http_req_blocked..............: avg=1ms    min=0s      med=0s    max=15ms
http_req_connecting.........: avg=2ms    min=0s      med=0s    max=10ms
http_req_duration...........: avg=45ms   min=12ms    med=38ms   max=180ms
  { expected_response:true }...: avg=45ms   min=12ms    med=38ms   max=180ms
http_req_failed..............: 0.00%   ✓ 5000      ✗ 0
http_req_receiving...........: avg=1ms    min=0s      med=0s    max=5ms
http_req_sending.............: avg=0s     min=0s      med=0s    max=1ms
http_req_tls_handshaking.....: avg=0s     min=0s      med=0s    max=0s
http_req_waiting.............: avg=43ms   min=11ms    med=37ms   max=175ms
http_reqs....................: 5000    26.221617/s
iteration_duration..........: avg=37ms   min=12ms    med=32ms   max=185ms
iterations...................: 5000    26.221617/s
vus........................: 50      min=50      max=50
vus_max....................: 50      min=50      max=50
```

## Interpreting Metrics

### Good Performance
- ✓ p(95) < 100ms for proxy requests
- ✓ p(95) < 50ms for direct backend requests
- ✓ Error rate < 1%
- ✓ Stable RPS (no drops)

### Performance Issues
- ✗ p(95) > 500ms
- ✗ Error rate > 5%
- ✗ Connection timeouts
- ✗ Memory leaks (increasing memory usage)

## Debugging Performance Issues

### Check Proxy Logs
```bash
# Look for slow requests
tail -f proxy.log | grep "Proxying to:"
```

### Monitor Backend
```bash
# Check backend is healthy
curl http://localhost:9001/
curl http://localhost:9002/
```

### System Resources
```bash
# CPU usage
top -pid $(pgrep proxy)

# Memory usage
ps aux | grep proxy

# Network connections
netstat -an | grep :8080
```

## Test Scenarios

### 1. Baseline Test (Direct to Backend)
```bash
# Test direct backend connection
curl http://localhost:9001/users
curl http://localhost:9002/orders
```

### 2. Proxy Test (Through Proxy)
```bash
# Test through proxy
curl http://localhost:8080/api/v1/users
curl http://localhost:8080/api/v1/orders
```

### 3. Concurrent Load Test
```javascript
export const options = {
    stages: [
        { duration: '10s', target: 100 },
        { duration: '30s', target: 1000 },
        { duration: '10s', target: 0 },
    ],
};
```

### 4. Stress Test (Find Breaking Point)
```javascript
export const options = {
    stages: [
        { duration: '10s', target: 100 },
        { duration: '20s', target: 1000 },
        { duration: '20s', target: 5000 },
        { duration: '10s', target: 0 },
    ],
};
```

## Advanced Features

### HTTP Headers
```javascript
const response = http.get('http://localhost:8080/api/v1/users', {
    headers: {
        'X-Test-Header': 'test-value',
    },
});
```

### POST Requests
```javascript
const response = http.post('http://localhost:8080/api/v1/users', JSON.stringify({
    name: 'John Doe',
}), {
    headers: { 'Content-Type': 'application/json' },
});
```

### Custom Metrics
```javascript
import { Trend, Counter } from 'k6/metrics';

const responseTime = new Trend('response_time');
const errorCount = new Counter('errors');

export default function () {
    const response = http.get('http://localhost:8080/api/v1/users');
    
    responseTime.add(response.timings.duration);
    
    if (response.status !== 200) {
        errorCount.add(1);
    }
}
```

## Best Practices

1. **Start Small**: Begin with 10-50 VUs
2. **Gradual Increase**: Ramp up slowly to find breaking point
3. **Monitor Resources**: Watch CPU, memory, and network
4. **Compare Metrics**: Always compare proxy vs direct backend
5. **Check Logs**: Look for errors and slow requests
6. **Test Endpoints**: Test each endpoint separately
7. **Run Multiple Times**: Verify results are consistent

## Troubleshooting

### "Connection Refused"
- Ensure proxy is running: `cargo run`
- Check port 8080 is free: `lsof -i :8080`

### "No matching directive for path"
- Check config file: `cat file.caddy`
- Verify path matches handle_path pattern

### High Error Rate
- Check backend health
- Verify proxy configuration
- Monitor system resources

### Slow Response Times
- Check backend performance
- Optimize proxy code
- Increase backend capacity