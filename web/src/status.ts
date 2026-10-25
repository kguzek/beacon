export const serviceDescriptions = {
  healthy: "The latest check from this location found the service working.",
  down: "The latest check from this location found a problem reaching the service.",
  unknown:
    "There isn’t a recent result from this location, so we can’t confirm the service’s status.",
};

export const combinedDescriptions = {
  healthy: "Every monitoring location recently found the service working.",
  down: "At least one location recently found a problem. Availability may vary by location.",
  unknown:
    "We don’t have enough recent results to confirm the service’s status.",
};

export const groupDescriptions = {
  healthy:
    "Every service in this group was recently confirmed working from every monitoring location.",
  down: "These services have a recent reported problem from at least one location. Availability may vary by location.",
  unknown:
    "These services are missing recent results, so we can’t confirm their status.",
};

export const overviewDescriptions: Record<string, string> = {
  "Checking service status…": "We’re waiting for the first status updates.",
  "All services online":
    "Every service was recently confirmed working from every monitoring location.",
  "All services down":
    "A problem was recently found with every service from at least one location. Availability may vary by location.",
  "Partial outage":
    "Some services have a reported problem. Others may be working or have an unknown status.",
  "Service status incomplete":
    "No problems have been reported, but some services are missing recent results.",
  "Service status unavailable":
    "We don’t have recent results to confirm how services are doing.",
};

export const monitorDescriptions = {
  connecting: "We’re waiting to hear from this monitoring location.",
  reachable:
    "We can receive updates from this location. Individual service results may still be out of date.",
  unreachable:
    "We can’t receive updates from this location right now. This doesn’t mean its services are down.",
};

export const healthStates = [
  { id: "healthy", label: "online" },
  { id: "down", label: "down" },
  { id: "unknown", label: "unknown" },
] as const;
