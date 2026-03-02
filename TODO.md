# TODO - Blacksky Community Posts

## Completed

- [x] Client-side CID computation before submission
- [x] Appview CID verification (expectedCid parameter)
- [x] CidMismatch error on verification failure
- [x] Rename contentHash to cid across codebase

## Remaining Work

### High Priority

- [ ] **Hydration-time CID verification in client**
  - Fetch stub record from user's PDS to get authoritative CID
  - Fetch hydrated content from appview
  - Compute CID from hydrated content
  - Compare: if mismatch, warn user that content may have been tampered
  - Location: `blacksky.community/src/state/queries/community-feed.ts`

- [ ] **Firehose listener for orphaned content cleanup**
  - Listen for delete events on `community.blacksky.feed.post` collection
  - When stub is deleted from PDS, delete corresponding content from appview's `community_post` table
  - Prevents orphaned content when user deletes stub directly via `deleteRecord`
  - Location: rsky-wintermute or separate service

### Medium Priority

- [ ] **Community post threadgate support**
  - Implement `community.blacksky.feed.threadgate` record type
  - Allow post authors to restrict who can reply
  - Rules: mentionRule, followerRule, followingRule, listRule
  - Lexicon exists at: `lexicons/community/blacksky/feed/threadgate.json`

- [ ] **Community feed aggregation**
  - Global community feed (all members' posts)
  - Filtered by engagement, recency, etc.
  - New endpoint: `community.blacksky.feed.getCommunityTimeline`

### Low Priority

- [ ] **Stub record verification on post fetch**
  - When fetching a community post, optionally verify stub exists in user's PDS
  - Ensures post wasn't created by appview without user's consent
  - Trade-off: adds latency, may not be necessary for all use cases

- [ ] **Content expiration policy**
  - Allow users to set expiration on community posts
  - Auto-delete content after expiration while keeping stub as tombstone
  - Useful for ephemeral content

## Notes

### On-Demand Hydration Pattern
The community posts follow the "on-demand record hydration" pattern:
1. Stub record in user's PDS: `{ createdAt, cid }`
2. Full content stored on appview
3. CID computed by client = source of truth for integrity
4. Appview hydrates stub with full content on fetch

### Integrity Guarantee
The CID in the stub is a cryptographic commitment:
- Computed by CLIENT from canonical record
- Stored in user's PDS (user controls)
- If appview modifies content, CID won't match
- Clients can verify by recomputing CID from hydrated content
