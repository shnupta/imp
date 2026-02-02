# Long-term Memory

*This file contains your agent's curated memories — important decisions, lessons learned, and key insights that should persist over time.*

## Key Decisions

### Technical Architecture
<!-- Important architectural decisions and their reasoning -->
- **Database choice**: Chose PostgreSQL over MongoDB because of strong consistency requirements and complex relational data (2024-01-15)
- **Authentication**: Implemented OAuth 2.0 with JWT tokens, refresh tokens stored in secure HttpOnly cookies (2024-01-20)

### Code Patterns & Standards  
<!-- Coding conventions and patterns adopted -->
- **Error handling**: Use Result<T, E> consistently, convert errors at boundaries with thiserror
- **API design**: RESTful endpoints, consistent JSON response format with data/error/meta fields
- **Testing strategy**: Integration tests for API endpoints, unit tests for business logic

## User Preferences

### Communication Style
<!-- How your user prefers to work and communicate -->
- Prefers direct feedback over diplomatic softening
- Likes detailed explanations for architectural decisions
- Wants to see code examples in suggestions

### Technical Preferences
<!-- User's preferred tools, languages, approaches -->
- Strong preference for Rust over Go for backend services
- Uses VS Code with Rust Analyzer, vim keybindings
- Prefers functional programming patterns where appropriate
- Likes exhaustive error handling

### Work Patterns
<!-- How your user works best -->
- Deep focus time: 9-11 AM, minimal interruptions preferred
- Code reviews: prefers async review with detailed comments
- Meetings: likes agenda and prep time, dislikes spontaneous calls

## Lessons Learned

### Development Process
<!-- Important insights about the development process -->
- **Testing databases**: Always use transactions in tests to avoid state pollution between test runs
- **Deployment**: Rolling deployments work better than blue/green for our current setup due to stateful connections
- **Code review**: Focus on logic and security; formatting is handled by automated tools

### Technical Insights
<!-- Technical lessons that should inform future decisions -->
- **Performance**: Database connection pooling reduced latency by 40% in production
- **Security**: Rate limiting prevented API abuse during traffic spikes
- **Monitoring**: Custom metrics for business logic proved more valuable than generic system metrics

## Project History

### Major Milestones
<!-- Significant project achievements and timeline -->
- **Q1 2024**: Authentication system launched, 99.9% uptime achieved
- **Q2 2024**: API rate limiting implemented after security review
- **Q3 2024**: Database migration to PostgreSQL 15 completed

### Known Issues & Workarounds
<!-- Current limitations and their temporary solutions -->
- **File upload size**: Limited to 10MB due to infrastructure constraints, users workaround with cloud storage links
- **Search performance**: Current implementation is acceptable but will need optimization at scale

## Team Dynamics

### Collaboration Patterns
<!-- How the team works together effectively -->
- **PR reviews**: Alice focuses on architecture, Bob on UX implications, Carol on deployment impact
- **Planning**: Weekly planning sessions work well, daily standups keep momentum
- **Knowledge sharing**: Tech talks every two weeks help spread domain knowledge

### Communication Preferences
<!-- How team members prefer to communicate -->
- **Alice**: Prefers written proposals for architecture changes, detailed RFC-style documents
- **Bob**: Visual thinker, appreciates diagrams and mockups in discussions
- **Carol**: Wants infrastructure impact assessment for all significant changes

---

*This memory grows over time as your agent learns. It helps maintain consistency and context across sessions, ensuring important insights aren't lost.*