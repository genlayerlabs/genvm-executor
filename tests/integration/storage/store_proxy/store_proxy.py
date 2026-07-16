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


class Contract(gl.contract.Contract):
	users: gl.storage.DynArray[User]

	def __init__(self):
		self.users.append(User('Ada', datetime.datetime.now()))
		user = self.users[-1]
		self.users[-1] = User('Definitely not Ada', datetime.datetime.now())
		print(user.name)
		assert user.name == 'Definitely not Ada'  # this is true!
