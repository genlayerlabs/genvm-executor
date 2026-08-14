import inspect

from genlayer.contract import Proxy, _ContractAt
from genlayer.vm.public_abi import StorageType


def test_storage_type_decided_names_preserve_wire_values_and_legacy_aliases():
	assert StorageType.LATEST_FINALIZED.value == 1
	assert StorageType.LATEST_DECIDED.value == 2
	assert StorageType.LATEST_FINAL is StorageType.LATEST_FINALIZED
	assert StorageType.LATEST_NON_FINAL is StorageType.LATEST_DECIDED


def test_contract_views_default_to_latest_decided():
	assert inspect.signature(Proxy.view).parameters['state'].default is StorageType.LATEST_DECIDED
	assert inspect.signature(_ContractAt.view).parameters['state'].default is StorageType.LATEST_DECIDED
