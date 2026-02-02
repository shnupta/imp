# Local Environment Notes

*This file contains environment-specific details that help your agent understand your setup.*

## Development Environment

### Project Locations
<!-- Where are your main projects? -->
- **Main repos**: ~/code/ or ~/projects/
- **Work projects**: <!-- Company projects location -->
- **Personal projects**: <!-- Personal projects location -->

### SSH & Remote Access
<!-- Servers and remote machines you use -->
```
### SSH Hosts
- production → user@prod-server.company.com
- staging → user@staging.company.com  
- development → localhost:2222 (Docker)
```

### Local Services
<!-- Local development services -->
- **Database**: PostgreSQL on port 5432, user: dev
- **Redis**: localhost:6379
- **Docker**: running on default socket
- **IDE**: VS Code with Rust Analyzer extension

## Team & Communication

### Team Members
<!-- Key people and their areas -->
- **Alice** (Tech Lead): Architecture decisions, system design
- **Bob** (Frontend): React, TypeScript, user experience  
- **Carol** (DevOps): CI/CD, deployments, infrastructure

### Communication Channels
<!-- Where team discussions happen -->
- **Slack**: #engineering for general, #alerts for incidents
- **Email**: engineering@company.com for formal communications
- **Video**: Zoom for meetings, gather.town for casual

## Project Context

### Current Sprint/Milestone
<!-- What are you working on right now? -->
- **Sprint goal**: Implement user authentication system
- **Deadlines**: Authentication MVP due next Friday
- **Blockers**: Waiting for security review of OAuth implementation

### Key Repositories  
<!-- Important repos and their purposes -->
- **backend-api**: Main REST API (Rust + Actix)
- **frontend-app**: React web app (TypeScript + Vite)
- **shared-types**: Common types and schemas
- **infrastructure**: Terraform and deployment configs

## Preferences & Shortcuts

### Workflow Preferences
- **Testing**: Write tests first, prefer integration tests for APIs
- **Commits**: Conventional commits format (feat:, fix:, docs:)
- **PRs**: Squash merges, descriptive titles, link to issues
- **Code review**: Focus on logic and security, style is automated

### Useful Commands & Scripts
```bash
### Common Tasks
- make dev-setup     # Set up local development environment  
- make test-all      # Run full test suite
- make lint-fix      # Auto-fix linting issues
- ./scripts/deploy staging  # Deploy to staging environment
```

## Tools & Accounts

### Development Tools
- **Package manager**: Cargo for Rust, npm for Node.js
- **Database tools**: pgcli for PostgreSQL, Redis CLI
- **API testing**: Postman collections in ./docs/api/
- **Monitoring**: Grafana dashboards, error tracking in Sentry

### External Services
<!-- Services and their purposes (no credentials!) -->
- **GitHub**: Code hosting, issue tracking, CI/CD actions
- **AWS**: Production hosting (ask DevOps for access)
- **Slack**: Team communication
- **Jira**: Project management and issue tracking

---

*Your agent uses this file to understand your specific setup and preferences. Update it as your environment changes or you discover new patterns.*