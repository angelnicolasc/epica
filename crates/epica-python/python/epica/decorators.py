"""Pythonic decorators for belief-governed classes and functions.

``@belief_state`` makes any class belief-governed by attaching a ``BeliefQuad``
and optional ``BehavioralContract`` to it::

    from epica import BehavioralContract
    from epica.decorators import belief_state, governed_by

    safety = BehavioralContract("safety")
    safety.add_precondition("user_approved", min_confidence=0.9)

    @belief_state(contract=safety)
    class Deployer:
        def __init__(self, env: str) -> None:
            self.belief_quad.insert("environment", env, 1.0)
            self.belief_quad.insert("user_approved", "yes", 0.95)

        @governed_by(safety)
        def deploy(self) -> None:
            print("deploying...")
"""

from __future__ import annotations

import functools
from typing import Any, Callable, Optional, Type, TypeVar

from epica._epica import BeliefQuad, BehavioralContract, ContractViolationError

F = TypeVar("F", bound=Callable[..., Any])
C = TypeVar("C", bound=type)


def belief_state(
    contract: Optional[BehavioralContract] = None,
) -> Callable[[Type[C]], Type[C]]:
    """Class decorator that attaches a ``BeliefQuad`` to every instance.

    After decoration, every instance of the class has:

    - ``self.belief_quad`` — a fresh ``BeliefQuad``
    - ``self.check_invariants()`` — evaluates the contract's invariants
    - ``self._belief_contract`` — the attached contract (or ``None``)

    If a ``contract`` is provided, ``check_preconditions_raising()`` is called
    at the end of ``__init__``.

    Args:
        contract: Optional ``BehavioralContract`` to enforce on the class.

    Returns:
        A class decorator.
    """
    def decorator(cls: Type[C]) -> Type[C]:
        original_init = cls.__init__  # type: ignore[misc]

        @functools.wraps(original_init)
        def new_init(self: Any, *args: Any, **kwargs: Any) -> None:
            self.belief_quad = BeliefQuad()
            self._belief_contract: Optional[BehavioralContract] = contract
            original_init(self, *args, **kwargs)
            if contract is not None:
                contract.check_preconditions_raising(self.belief_quad)

        def check_invariants(self: Any) -> bool:
            if self._belief_contract is None:
                return True
            return self._belief_contract.check_invariants(self.belief_quad)

        cls.__init__ = new_init  # type: ignore[method-assign]
        cls.check_invariants = check_invariants  # type: ignore[attr-defined]
        return cls

    return decorator


def governed_by(contract: BehavioralContract) -> Callable[[F], F]:
    """Function/method decorator that enforces a ``BehavioralContract``.

    Before the call: ``contract.check_preconditions_raising(self.belief_quad)``
    After the call:  ``contract.check_invariants_raising(self.belief_quad)``

    The decorated function must be a method on a class decorated with
    ``@belief_state`` (so that ``self.belief_quad`` is available).

    Raises:
        ``ContractViolationError``: when a precondition or invariant fails.

    Example::

        @governed_by(safety_contract)
        def deploy(self) -> None:
            ...
    """
    def decorator(func: F) -> F:
        @functools.wraps(func)
        def wrapper(self: Any, *args: Any, **kwargs: Any) -> Any:
            quad = getattr(self, "belief_quad", None)
            if quad is None:
                raise AttributeError(
                    f"@governed_by requires the class to have a 'belief_quad' attribute. "
                    f"Decorate the class with @belief_state first."
                )
            contract.check_preconditions_raising(quad)
            result = func(self, *args, **kwargs)
            contract.check_invariants_raising(quad)
            return result

        return wrapper  # type: ignore[return-value]

    return decorator
