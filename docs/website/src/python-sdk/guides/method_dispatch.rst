.. _method-dispatch:

Public, Payable and Special Methods
===================================

A contract method is private unless it is decorated. The decorator decides who may
call it, whether it may change state and whether a call may carry value.

Decorators
----------

.. list-table::
   :header-rows: 1

   * - Decorator
     - Callable by name
     - Changes state
     - Accepts value
   * - none, or ``gl.private``
     - no
     - —
     - —
   * - ``gl.public.view``
     - yes
     - no
     - no
   * - ``gl.public.write``
     - yes
     - yes
     - no
   * - ``gl.public.write.payable``
     - yes
     - yes
     - yes

``gl.public.write.min_gas(leader=..., validator=...)`` is ``gl.public.write`` that also
declares the minimal gas a call needs; it has ``.payable`` too.

A call that carries value to a method that is not payable is refused, and so is a
call to a private method.

.. code-block:: python

    class Contract(gl.contract.Contract):
        @gl.public.view
        def get(self) -> int: ...

        @gl.public.write.payable
        def deposit(self) -> None: ...

Special Methods
---------------

The VM calls 4 methods on its own. None of them can be called by name, and decorating
any of them with ``gl.public`` raises ``TypeError`` when the class is defined.

``__init__``
   The constructor, run once on deploy

``__receive__``
   A plain value transfer, i.e. a message without a method name. It is always payable

``__handle_undefined_method__``
   A call without value to a method the contract does not define. Without an override it
   is handled by ``__handle_undefined_method_payable__``

``__handle_undefined_method_payable__``
   A call with value to a method the contract does not define. Without an override such a
   call is refused

Both fallbacks receive the method name, positional and keyword arguments:

.. code-block:: python

    class Contract(gl.contract.Contract):
        def __receive__(self):
            self.received += gl.message.value

        def __handle_undefined_method_payable__(
            self, method_name: str, args: list, kwargs: dict
        ):
            ...

A contract that overrides only ``__handle_undefined_method_payable__`` handles calls
to undefined methods both with and without value.

Dispatch
--------

.. mermaid::

   graph TD
      Msg["Message"] --> Init{"Deploy?"}
      Init -- yes --> Ctor["__init__"]
      Init -- no --> Name{"Method name"}
      Name -- "empty" --> HasRecv{"__receive__ defined?"}
      HasRecv -- yes --> Recv["__receive__"]
      HasRecv -- no --> Value
      Name -- "starts with __" --> Refuse["Refused"]
      Name -- "other" --> Defined{"Method defined?"}
      Defined -- yes --> Check{"Public, and payable if value is non-zero?"}
      Check -- yes --> Meth["The method"]
      Check -- no --> Refuse
      Defined -- no --> Value{"Value is non-zero?"}
      Value -- no --> HasFb{"__handle_undefined_method__ defined?"}
      HasFb -- yes --> Fb["__handle_undefined_method__"]
      HasFb -- no --> HasPay
      Value -- yes --> HasPay{"__handle_undefined_method_payable__ defined?"}
      HasPay -- yes --> FbPay["__handle_undefined_method_payable__"]
      HasPay -- no --> Refuse

A plain transfer to a contract without ``__receive__`` reaches the fallbacks with an
empty method name.
