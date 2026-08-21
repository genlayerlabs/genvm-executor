Migration Guide
===============

This page covers 2 migrations:

* :ref:`py-sdk-v03-rc` — a contract that already targets an earlier v0.3 release candidate
* :ref:`py-sdk-v02-to-v03` — a contract written for v0.2

The first is a subset of the second: a contract coming from v0.2 gets all of it as part of the move

.. _py-sdk-v03-rc:

Within v0.3: Release-Candidate Changes
--------------------------------------

.. warning::
    v0.3 is not released yet, and these changes are breaking *within* v0.3: a contract that ran on an earlier release candidate needs them. They are also folded into the v0.2 sections below

Pre-finalization State Is ``decided``
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

The state before finalization is named *decided* everywhere: the ``on`` argument of write calls and deploys takes ``'decided'`` instead of ``'accepted'``, and the storage states are renamed:

.. list-table::
   :header-rows: 1
   :widths: 50 50

   * - before
     - now
   * - ``on='accepted'``
     - ``on='decided'``
   * - ``StorageType.LATEST_NON_FINAL``
     - ``StorageView.LATEST_DECIDED``
   * - ``StorageType.LATEST_FINAL``
     - ``StorageView.LATEST_FINALIZED``

Sandboxes and Runners
~~~~~~~~~~~~~~~~~~~~~~

``gl.vm.spawn_sandbox`` lost both the ``runner`` and the ``allow_register_runners`` parameters:

.. code-block:: python

    # before
    gl.vm.spawn_sandbox(fn, runner=rid, allow_register_runners=True)

    # now
    gl.vm.spawn_runner(rid, calldata.encode(payload))

* to run a runner other than this contract's own one, use ``gl.vm.spawn_runner(runner, data)``; it takes the entry payload as bytes rather than a pickled callable, so the child need not be Python
* instead of a permission flag, the set of visible ``custom:<hash>`` runners is passed explicitly as ``custom_runners``: ``None`` (default) grants this VM's whole set, a list grants exactly that subset of it. It is accepted by ``spawn_runner``, ``spawn_sandbox``, ``run_nondet`` and ``run_nondet_default``
* ``changes_on_error`` was added to the sandbox spawners; ``'inherit'`` is the only value for now

Runner ids are typed as ``gl.vm.RunnerID``, and ``gl.vm.RunnerIDOps.new_chain(addr, state, slot_id)`` builds a ``chain:`` one. ``gl.vm.register_runner`` returns a ``RunnerID``, no longer requires a dedicated permission, and accepts a zip, a raw wasm module or commented text — a ustar archive is no longer a valid input.

Catching VM Errors
~~~~~~~~~~~~~~~~~~~

A ``catch_vm_error`` flag makes the callee's VM error a value instead of a re-raise; a fatal error is never caught. It is accepted by ``Proxy.view``, ``gl.vm.run_nondet`` and ``gl.vm.run_nondet_default``:

.. code-block:: python

    res = gl.contract.get_at(addr).view(catch_vm_error=True).balance_of(owner)

Calldata
~~~~~~~~

* ``calldata.Raw(data)`` splices an already encoded blob into the output verbatim; nothing validates it
* ``calldata.DataclassMixin`` encodes a dataclass as a map of field name to value
* a ``memoryview`` is now encoded as ``bytes``, like ``bytes``/``bytearray``. Previously its contents were spliced in as raw calldata — use ``calldata.Raw`` if that was the intent
* ``calldata.to_str`` prints ``raw#<hex>`` for ``Raw`` and ``b#<hex>`` for any buffer
* ``Decoded`` now also lists ``Address`` and ``bool``, which decoding could always produce

VM Error Codes
~~~~~~~~~~~~~~~

``gl.vm.ABI`` error codes were restructured, and codes that carry a detail suffix are now nested:

.. list-table::
   :header-rows: 1
   :widths: 50 50

   * - before
     - now
   * - ``absent_leader_nondet_output``
     - ``leader_fault nondet_output absent``
   * - ``host_forbidden``
     - ``forbidden``
   * - ``invalid_contract absent_runner_comment``
     - ``invalid_contract runner absent``
   * - ``invalid_contract malformed_runner``
     - ``invalid_contract runner malformed``

``malformed_entry`` is new, ``out_of receipt message``, ``out_of message_fee total``, ``out_of message_fee allocation_budget`` and ``fee no_matching_allocation`` gained ``internal``/``external`` variants, and ``ResultCode.INTERNAL_ERROR`` is gone. The ``memory_limiter_consts`` and ``top_limits`` tables were removed from ``public_abi``.

Storage
~~~~~~~

* ``DynArray`` slice assignment, ``VLA.extend`` and ``VLA.assign`` accept any iterable, not only a sequence
* generic parameters are resolved through base classes, so a subclass of a generic storage class no longer fails to build
* a recursive storage type is reported as an error instead of building an incomplete layout
* an ``Array`` or ``ndarray`` dimension must be strictly positive, and the total size must fit the 32-bit storage address space

Smaller Fixes
~~~~~~~~~~~~~

.. list-table::
   :header-rows: 1
   :widths: 50 50

   * - before
     - now
   * - ``gl.evm.Account.emit_value(value, data)``
     - ``gl.evm.Account.emit_call(value, data)``
   * - ``gl.nondet.exec_prompt(..., image=...)``
     - ``gl.nondet.exec_prompt(..., images=...)``

``value`` of ``gl.contract.deploy`` defaults to ``0``, a nested ``Annotated`` is unwrapped correctly during schema generation, and malformed ``args``/``kwargs`` in the entry calldata raise a ``TypeError`` instead of failing later

.. _py-sdk-v02-to-v03:

v0.2.x to v0.3.0
----------------

v0.3.0 introduces a major restructuring of the standard library. The ``genlayer.gl`` and ``genlayer.py`` intermediate packages are removed. All public API is now accessible directly under the ``genlayer`` namespace.

Import Pattern
~~~~~~~~~~~~~~~

The recommended import pattern has changed:

.. code-block:: python

    # v0.2.x
    from genlayer import *

    # v0.3.0
    import genlayer as gl
    from genlayer.types import *

The ``from genlayer import *`` star-import previously brought ``gl`` (a lazy proxy to ``genlayer.gl``), all types, and storage names into scope. Now ``import genlayer as gl`` gives you direct access to submodules (``gl.contract``, ``gl.vm``, ``gl.message``, ``gl.chain``, ``gl.storage``, etc.) and decorators (``gl.public``, ``gl.private``). The ``from genlayer.types import *`` import brings the type aliases (``u8``..``u256``, ``i8``..``i256``, ``Address``, ``bigint``, etc.) into local scope.

Module Path Changes
~~~~~~~~~~~~~~~~~~~~~

All ``genlayer.py.*`` and ``genlayer.gl.*`` paths are removed. Here is the mapping:

.. list-table::
   :header-rows: 1
   :widths: 50 50

   * - v0.2.x
     - v0.3.0
   * - ``genlayer.py.types``
     - ``genlayer.types``
   * - ``genlayer.py.keccak``
     - ``genlayer.types.keccak``
   * - ``genlayer.py.calldata``
     - ``genlayer.calldata``
   * - ``genlayer.py.storage``
     - ``genlayer.storage``
   * - ``genlayer.py.evm``
     - ``genlayer.evm``
   * - ``genlayer.gl.vm``
     - ``genlayer.vm``
   * - ``genlayer.gl.nondet``
     - ``genlayer.nondet``
   * - ``genlayer.gl.eq_principle``
     - ``genlayer.eq_principle``
   * - ``genlayer.gl.genvm_contracts``
     - ``genlayer.contract``
   * - ``genlayer.gl.annotations``
     - ``genlayer._internal.annotations``
   * - ``genlayer.gl.advanced``
     - removed (see below)

.. note::
    Type aliases (``u8``..``u256``, ``i8``..``i256``) are no longer ``typing.NewType`` instances; they are now ``typing.Annotated[int, ...]``. They still work as type annotations, but they are no longer callable, so the ``u256(0)`` wrapping is gone — pass a plain ``int`` instead:

    .. code-block:: python

        # v0.2.x
        gl.contract.deploy(code=source, value=u256(1000), salt_nonce=u256(42))

        # v0.3.0
        gl.contract.deploy(code=source, value=1000, salt_nonce=42)

Contract Declaration
~~~~~~~~~~~~~~~~~~~~~

``gl.Contract`` is now ``gl.contract.Contract``:

.. code-block:: python

    # v0.2.x
    class MyToken(gl.Contract):
        ...

    # v0.3.0
    class MyToken(gl.contract.Contract):
        ...

Contract Interaction
~~~~~~~~~~~~~~~~~~~~~

Functions for interacting with other contracts have been renamed and moved into ``gl.contract``:

.. code-block:: python

    # v0.2.x
    contract = gl.get_contract_at(address)
    gl.deploy_contract(code=source, args=[...])

    @gl.contract_interface
    class IToken:
        class View:
            def balance_of(self, owner: Address) -> u256: ...

    # v0.3.0
    contract = gl.contract.get_at(address)
    gl.contract.deploy(code=source, args=[...])

    @gl.contract.interface
    class IToken:
        class View:
            def balance_of(self, owner: Address) -> u256: ...

The ``ContractProxy`` type is renamed to ``gl.contract.Proxy``.

GenLayer contract proxies, ``Contract`` itself, and the new ``gl.chain.Account`` all implement the ``gl.chain.IAccount`` protocol, exposing ``address``, ``balance``, and ``emit_transfer(value, *, on=...)``. EVM contract proxies expose ``emit_transfer(value)`` without ``on`` because ``EmitExternalMessage`` has no decided/finalized staging option.

Value Transfers and ``on=`` Parameter
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Write-call and deploy APIs now take an ``on`` keyword controlling when the message is applied, with values ``'decided'`` or ``'finalized'`` (default ``'finalized'``):

.. code-block:: python

    contract.emit(value=100, on='finalized').transfer(to, amount)
    gl.contract.deploy(code=source, value=1000, on='finalized', salt_nonce=42)

A new ``emit_transfer`` helper sends a plain value transfer without a method call. The receiver may catch it via ``Contract.__receive__`` (must be ``@gl.public.write.payable``):

.. code-block:: python

    contract.emit_transfer(1000, on='finalized')

Accounts
~~~~~~~~

``gl.chain.Account`` is a new lightweight wrapper around an ``Address`` that allows querying the balance of, or emitting a transfer to, any on-chain account (contract or EoA):

.. code-block:: python

    acc = gl.chain.Account(some_address)
    bal = acc.balance
    acc.emit_transfer(100)

Message Context
~~~~~~~~~~~~~~~

The ``gl.message`` object was a ``NamedTuple``. It is now a module (``genlayer.message``) with the same fields as module-level attributes:

.. code-block:: python

    # v0.2.x
    sender = gl.message.sender_address
    value = gl.message.value

    # v0.3.0 (identical usage, but gl.message is a module now)
    sender = gl.message.sender_address
    value = gl.message.value

``gl.message_raw`` is now ``gl.message.raw``. The chain ID is exposed as ``gl.message.chain_id`` (and as ``gl.chain.id``).

Events
~~~~~~

The ``Event`` class has moved from ``genlayer.gl`` to ``genlayer.chain``:

.. code-block:: python

    # v0.2.x
    class Transfer(gl.Event):
        def __init__(self, sender: Address, to: Address, /): ...

    # v0.3.0
    class Transfer(gl.chain.Event):
        def __init__(self, sender: Address, to: Address, /): ...

``gl.advanced.emit_raw_event(topics, blob)`` is now ``gl.chain.Event.emit_raw(topics, blob)``.

Advanced / Error Handling
~~~~~~~~~~~~~~~~~~~~~~~~~~~

The ``genlayer.gl.advanced`` module is removed. Its functionality has been relocated:

.. code-block:: python

    # v0.2.x
    gl.advanced.user_error_immediate("reason")
    gl.advanced.emit_raw_event(topics, blob)

    # v0.3.0
    gl.vm.UserError.immediate("reason")
    gl.chain.Event.emit_raw(topics, blob)

``UserError`` now carries an arbitrary calldata-encodable payload instead of a string. The payload is accessed via ``.data`` (previously ``.message``), and ``UserError.immediate`` accepts any ``calldata.Encodable``:

.. code-block:: python

    raise gl.vm.UserError({'kind': 'InsufficientBalance', 'have': have, 'need': need})

    try:
        ...
    except gl.vm.UserError as e:
        payload = e.data

The error-message handler hook (``__on_errored_message__``) has been removed.

VM Tracing
~~~~~~~~~~

Trace functions have moved from ``genlayer.gl`` to ``genlayer.vm``:

.. code-block:: python

    # v0.2.x
    gl.trace("debug message")
    gl.trace_time_micro()

    # v0.3.0
    gl.vm.trace("debug message")
    gl.vm.trace_time_micro()

Non-deterministic Execution (``gl.vm``)
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

.. warning::
    ``gl.vm.run_nondet`` **changed meaning**. The two ``run_nondet`` functions were renamed:

    * the old ``run_nondet_unsafe`` (no validator sandbox) is now ``run_nondet``;
    * the old ``run_nondet`` (safe, validator runs in a sandbox) is now ``run_nondet_default``.

    Code that called ``gl.vm.run_nondet`` still compiles but now silently uses the
    *unsafe* variant. To keep the previous behavior, switch to ``gl.vm.run_nondet_default``:

    .. code-block:: python

        # v0.2.x (safe)            # v0.3.0 (same behavior)
        gl.vm.run_nondet(...)      gl.vm.run_nondet_default(...)

        # v0.2.x (unsafe)          # v0.3.0 (same behavior)
        gl.vm.run_nondet_unsafe(...)   gl.vm.run_nondet(...)

The high-level equivalence principles (``gl.eq_principle.*``) are unaffected — they were updated internally.

``gl.vm.spawn_sandbox`` replaced the single ``allow_write_ops`` flag with granular permissions, each effective only if the current VM holds it:

.. code-block:: python

    # v0.2.x
    gl.vm.spawn_sandbox(fn, allow_write_ops=True)

    # v0.3.0
    gl.vm.spawn_sandbox(
        fn,
        allow_write_storage=True,
        allow_send_messages=True,
    )

``gl.vm.spawn_sandbox`` always runs this contract's own runner. To run a different one -- which need not be Python -- use ``gl.vm.spawn_runner(runner, data)``, which takes the entry payload as bytes instead of a pickled callable; ``spawn_sandbox`` is a wrapper over it.

Three runtime helpers were added: ``gl.vm.register_runner(code)`` registers a runner archive and returns its ``custom:<hash>`` id, ``gl.vm.map_file(runner, path_in_runner, path_in_vfs)`` maps a file from a runner into the VM filesystem, and ``gl.vm.spawn_runner`` runs one in a sandbox.

Which runners a child VM sees is controlled by ``custom_runners``, whether it may keep its changes by ``changes_on_error``, and whether a VM error of a callee becomes a value by ``catch_vm_error``; see :ref:`py-sdk-v03-rc`.

Calldata
~~~~~~~~

``genlayer.py.calldata`` is now ``genlayer.calldata``, and it gained ``calldata.Raw`` for splicing an already encoded blob and ``calldata.DataclassMixin`` for encoding a dataclass as a map; see :ref:`py-sdk-v03-rc`.

Storage
~~~~~~~

Storage types (``DynArray``, ``Array``, ``TreeMap``) and the ``allow`` decorator are accessible via ``gl.storage``:

.. code-block:: python

    # v0.2.x
    x: gl.DynArray[str]
    m: gl.TreeMap[str, u32]
    @gl.allow
    class MyRecord: ...

    # v0.3.0
    x: gl.storage.DynArray[str]
    m: gl.storage.TreeMap[str, u32]
    @gl.storage.allow
    class MyRecord: ...

``allow_storage`` was renamed to ``allow`` (accessible as ``gl.storage.allow``). A new ``gl.storage.Pickled[T]`` helper is available for storing arbitrary picklable objects.

Decorators
~~~~~~~~~~

``gl.public`` and ``gl.private`` are still available directly on ``gl``:

.. code-block:: python

    # Both v0.2.x and v0.3.0
    @gl.public.write
    def transfer(self, to: Address, amount: u256): ...

    @gl.public.view
    def balance_of(self, owner: Address) -> u256: ...

Non-deterministic Operations
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

``gl.nondet`` now raises a dedicated ``gl.nondet.NondetException`` (with ``causes`` and ``ctx``) on errors instead of a bare exception. Web request helpers (``gl.nondet.web.get``/``post``/``put``/...) accept a ``sign: bool`` keyword to sign outbound requests with the contract's identity.

Environment Detection
~~~~~~~~~~~~~~~~~~~~~~

A new top-level ``gl.IS_IN_VM`` boolean indicates whether code is running inside the GenVM, which is useful for code that is shared between contracts and off-chain tooling. The raw WASI module is available as ``gl.wasi``. A ``gl.gvm32`` module provides Crockford Base32 ``encode``/``decode`` helpers (mirroring the Rust ``genlayer_sdk::gvm32`` implementation).

Summary of Renames
~~~~~~~~~~~~~~~~~~~

.. list-table::
   :header-rows: 1
   :widths: 50 50

   * - v0.2.x
     - v0.3.0
   * - ``gl.Contract``
     - ``gl.contract.Contract``
   * - ``gl.contract_interface``
     - ``gl.contract.interface``
   * - ``gl.deploy_contract(...)``
     - ``gl.contract.deploy(...)``
   * - ``gl.get_contract_at(addr)``
     - ``gl.contract.get_at(addr)``
   * - ``gl.ContractProxy``
     - ``gl.contract.Proxy``
   * - ``gl.Event``
     - ``gl.chain.Event``
   * - ``gl.advanced.user_error_immediate(...)``
     - ``gl.vm.UserError.immediate(...)``
   * - ``gl.advanced.emit_raw_event(...)``
     - ``gl.chain.Event.emit_raw(...)``
   * - ``gl.trace(...)``
     - ``gl.vm.trace(...)``
   * - ``gl.trace_time_micro()``
     - ``gl.vm.trace_time_micro()``
   * - ``gl.vm.run_nondet_unsafe(...)``
     - ``gl.vm.run_nondet(...)``
   * - ``gl.vm.run_nondet(...)``
     - ``gl.vm.run_nondet_default(...)``
   * - ``gl.message_raw``
     - ``gl.message.raw``
   * - ``gl.storage.allow_storage``
     - ``gl.storage.allow``
   * - ``gl.allow``
     - ``gl.storage.allow``
   * - ``gl.DynArray``
     - ``gl.storage.DynArray``
   * - ``gl.Array``
     - ``gl.storage.Array``
   * - ``gl.TreeMap``
     - ``gl.storage.TreeMap``
   * - ``UserError(msg: str).message``
     - ``UserError(data: Encodable).data``

Renames made between v0.3 release candidates are listed separately in :ref:`py-sdk-v03-rc`
