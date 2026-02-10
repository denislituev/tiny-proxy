import http from "k6/http";
import { check, sleep } from "k6";

// Test configuration
export const options = {
  stages: [
    { duration: "30s", target: 10 }, // Ramp up to 10 users
    { duration: "1m", target: 50 }, // Stay at 50 users
    { duration: "30s", target: 100 }, // Ramp up to 100 users
    { duration: "1m", target: 100 }, // Stay at 100 users
    { duration: "30s", target: 0 }, // Ramp down to 0
  ],
  thresholds: {
    http_req_duration: ["p(95)<500"], // 95% of requests < 500ms
    http_req_failed: ["rate<0.05"], // < 5% error rate
  },
};

// Proxy endpoints
const PROXY_URL_USERS = "http://localhost:8080/api/v1/users";
const PROXY_URL_ORDERS = "http://localhost:8080/api/v1/orders";
const PROXY_URL_HEALTH = "http://localhost:8080/health";

// Direct backend endpoints (for comparison)
const DIRECT_URL_USERS = "http://localhost:9001/users";
const DIRECT_URL_ORDERS = "http://localhost:9002/orders";

export default function () {
  // Randomly select endpoint
  const scenarios = [
    { url: PROXY_URL_USERS, name: "proxy-users" },
    { url: PROXY_URL_ORDERS, name: "proxy-orders" },
    { url: PROXY_URL_HEALTH, name: "proxy-health" },
    { url: DIRECT_URL_USERS, name: "direct-users" },
    { url: DIRECT_URL_ORDERS, name: "direct-orders" },
  ];

  const scenario = scenarios[Math.floor(Math.random() * scenarios.length)];

  // Make request
  const response = http.get(scenario.url, {
    tags: { name: scenario.name },
  });

  // Validate response
  check(response, {
    [`${scenario.name} status is 200`]: (r) => r.status === 200,
    [`${scenario.name} response time < 500ms`]: (r) => r.timings.duration < 500,
    [`${scenario.name} has body`]: (r) => r.body.length > 0,
  });

  // Small pause between requests
  sleep(Math.random() * 0.5 + 0.1); // 0.1-0.6s random pause
}

// Setup function - runs once before test
export function setup() {
  console.log("Starting load test...");
  console.log(`Testing proxy at ${PROXY_URL_USERS}`);
}

// Teardown function - runs once after test
export function teardown(data) {
  console.log("Load test completed!");
}
