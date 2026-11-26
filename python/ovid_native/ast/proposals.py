import asyncio
from time import monotonic

from ovid_native.ast.errors import AstProposalExpiredError, AstProposalNotFoundError
from ovid_native.ast.models import AstLimits


class AstProposalStore[Proposal]:
    def __init__(self, limits: AstLimits) -> None:
        self._limits = limits
        self._proposals: dict[str, tuple[Proposal, float]] = {}
        self._lock = asyncio.Lock()

    async def put(self, proposal_id: str, proposal: Proposal) -> None:
        now = monotonic()
        async with self._lock:
            self._purge_expired(now)
            while len(self._proposals) >= self._limits.max_pending_proposals:
                del self._proposals[next(iter(self._proposals))]
            self._proposals[proposal_id] = (proposal, now + self._limits.proposal_ttl_seconds)

    async def get(self, proposal_id: str) -> Proposal:
        async with self._lock:
            entry = self._proposals.get(proposal_id)
        if entry is None:
            raise AstProposalNotFoundError(f'AST rewrite proposal not found: {proposal_id}')

        return entry[0]

    async def take(self, proposal_id: str) -> Proposal:
        entry, now = await self._pop(proposal_id)
        if entry is None:
            raise AstProposalNotFoundError(f'AST rewrite proposal not found: {proposal_id}')

        proposal, expires_monotonic = entry
        if expires_monotonic <= now:
            raise AstProposalExpiredError(f'AST rewrite proposal expired: {proposal_id}')

        return proposal

    async def discard(self, proposal_id: str) -> bool:
        entry, now = await self._pop(proposal_id)
        return entry is not None and entry[1] > now

    async def _pop(self, proposal_id: str) -> tuple[tuple[Proposal, float] | None, float]:
        async with self._lock:
            now = monotonic()
            entry = self._proposals.pop(proposal_id, None)
            self._purge_expired(now)

        return entry, now

    def _purge_expired(self, now: float) -> None:
        expired = [key for key, (_, expires_monotonic) in self._proposals.items() if expires_monotonic <= now]
        for key in expired:
            del self._proposals[key]
