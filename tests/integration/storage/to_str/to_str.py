# { "Depends": "py-genlayer:test" }

import datetime
from dataclasses import dataclass

import genlayer as gl
from genlayer.storage import allow


@allow
@dataclass
class User:
	name: str
	birthday: datetime.datetime


class LlmErc20(gl.contract.Contract):
	x: User

	def __init__(self) -> None:
		print(str(self.x))
